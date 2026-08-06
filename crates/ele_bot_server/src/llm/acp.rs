//! ACP (Agent Client Protocol) 客户端
//!
//! 基于官方 SDK `agent-client-protocol` (v2) spawn 并连接 `zeroclaw acp` 子进程;
//! 协议编解码 / 请求路由 / 审批响应由 SDK 接管, 本模块只负责:
//! 长连接生命周期管理 / 流式分片聚合 / 工具审批策略 / 超时控制.
//!
//! # 长连接模型
//! SDK 的 `Client::connect_with` 前台 future 一返回就关闭连接, 因此 `spawn`
//! 把 `ConnectionTo<Agent>` handle 经 oneshot 传出, 前台 future 挂在关闭信号上
//! 保持连接长驻; 整个连接 (SDK 事件循环 + 子进程监控) 由一个 tokio task 驱动.
//! `ConnectionTo` 可廉价 Clone, `send_request(..).block_task()` 只是等待一个
//! oneshot 响应 — 在 dispatch loop 之外的任意 task 调用都安全 (SDK 文档明确
//! 允许), 唯一禁止的是在 handler 回调里 await, 本模块的 handler 都不等待响应.
//!
//! # 子进程管理
//! SDK 在 Unix 下把子进程设为独立进程组组长, 连接关闭时 SIGKILL 整个进程组 —
//! 等价于旧实现的 `kill_on_drop`, 且能覆盖 zeroclaw 再 fork 的孙进程.
//! `AcpClient` 被 drop 时关闭信号随之释放, 后台 task 自动走完关闭流程,
//! 不留孤儿进程. zeroclaw 的 stderr 由 SDK 捕获后经 debug callback 逐行转发
//! 到本进程 log (旧实现是 stderr 直接继承本进程, 效果等价但走 log 框架);
//! 子进程非零退出时 SDK 会把 stderr 尾部附进错误信息.
//!
//! # 多 session
//! `session_new` 返回的 session id 由调用方持有, `prompt`/`session_stop` 显式
//! 传入 — 一个连接可同时挂 chat 长驻 session 与 mood 临时 session.
//! 任何错误直接向上返回 `Err`, 由调用方 (`ZeroclawLlm`) 丢弃本实例整体重建:
//! SDK 连接侧没有需要清理的客户端状态 (超时未完成的 request 随连接关闭失效),
//! 重建即全新进程 + 全新连接, 与旧实现语义一致.
//!
//! # 工具审批
//! ACP 模式下 zeroclaw 把工具审批委托给客户端 (`session/request_permission`,
//! 必须回响应否则 agent 轮次挂起); 策略: 选第一个 id/name 含 "allow" 的选项
//! (实测 zeroclaw 提供 allow-once / allow-always, 排在前的是 allow-once,
//! 即每次单独授权, 不在 zeroclaw 侧固化永久放行规则); 找不到则回 Cancelled
//! 并告警. 可用工具与高危命令拦截由用户自己的 zeroclaw 配置 / risk_profile
//! 决定, 本客户端不越权否决.

use agent_client_protocol::schema::v1::{
    CloseSessionRequest, ContentBlock, InitializeRequest, NewSessionRequest, PermissionOption,
    PermissionOptionId, PromptRequest, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, SelectedPermissionOutcome, SessionId, SessionNotification,
    SessionUpdate, TextContent,
};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::{AcpAgent, AcpAgentConfig, Agent, Client, ConnectionTo, LineDirection};
use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// initialize 握手超时; zeroclaw 本地进程, 3s 足够
const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(3);
/// session/new 与 session/close 超时
const SESSION_TIMEOUT: Duration = Duration::from_secs(5);
/// kill 时等待连接关闭流程走完的上限 (SDK 关闭流程内含 1s 进程退出宽限)
const KILL_TIMEOUT: Duration = Duration::from_secs(5);

/// 从 `session/update` 通知提取 agent_message_chunk 的文本分片; 其它更新返回 None
fn extract_chunk_text(notification: &SessionNotification) -> Option<&str> {
    let SessionUpdate::AgentMessageChunk(chunk) = &notification.update else {
        return None;
    };
    match &chunk.content {
        ContentBlock::Text(text) => Some(&text.text),
        _ => None,
    }
}

/// 审批选项策略: 选第一个 id 或名称含 "allow" 的选项; 找不到返回 None
/// (调用方回 Cancelled). 纯函数便于单测.
fn choose_allow_option(options: &[PermissionOption]) -> Option<PermissionOptionId> {
    options
        .iter()
        .find(|o| {
            o.option_id.0.to_ascii_lowercase().contains("allow")
                || o.name.to_ascii_lowercase().contains("allow")
        })
        .map(|o| o.option_id.clone())
}

/// ACP 客户端: 持有一条到 `zeroclaw acp` 子进程的长连接
pub struct AcpClient {
    /// 长连接 handle (SDK 事件循环在后台 task 驱动)
    conn: ConnectionTo<Agent>,
    /// 各 session 的流式分片聚合缓冲 (notification handler 写入, prompt 读取)
    chunks: Arc<Mutex<HashMap<String, String>>>,
    /// 关闭信号: send (或 drop) 后前台 future 返回, 连接关闭并 SIGKILL 进程组
    shutdown: Option<oneshot::Sender<()>>,
    /// 驱动连接的 tokio task (connect_with future)
    task: Option<JoinHandle<Result<(), agent_client_protocol::Error>>>,
}

impl AcpClient {
    /// spawn `zeroclaw acp` 子进程并建立长连接
    ///
    /// 不传 `--config-dir`: zeroclaw 配置 (provider/api_key/人设) 完全由用户
    /// 自己维护 (默认 `~/.zeroclaw`), 本进程不介入渲染
    ///
    /// # Arguments
    ///
    /// * `bin` - zeroclaw 二进制路径
    pub async fn spawn(bin: &Path) -> Result<Self> {
        let chunks: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
        let chunks_cb = Arc::clone(&chunks);
        let builder = Client
            .builder()
            .name("ele-bot-server")
            .on_receive_notification(
                async move |n: SessionNotification, _cx| {
                    if let Some(text) = extract_chunk_text(&n) {
                        chunks_cb
                            .lock()
                            .expect("chunks 锁中毒")
                            .entry(n.session_id.to_string())
                            .or_default()
                            .push_str(text);
                    }
                    Ok(())
                },
                agent_client_protocol::on_receive_notification!(),
            )
            .on_receive_request(
                async move |req: RequestPermissionRequest, responder, _conn| {
                    let tool = req.tool_call.fields.title.unwrap_or_default();
                    match choose_allow_option(&req.options) {
                        Some(id) => {
                            log::debug!("zeroclaw 工具审批: {tool} -> {id}");
                            responder.respond(RequestPermissionResponse::new(
                                RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
                                    id,
                                )),
                            ))
                        }
                        None => {
                            log::warn!("zeroclaw 工具审批: {tool} 无 allow 选项, 回 Cancelled");
                            responder.respond(RequestPermissionResponse::new(
                                RequestPermissionOutcome::Cancelled,
                            ))
                        }
                    }
                },
                agent_client_protocol::on_receive_request!(),
            );

        // stderr 诊断逐行转发到本进程 log; 协议帧只进 trace
        let agent =
            AcpAgent::new(AcpAgentConfig::new(bin).arg("acp")).with_debug(|line, direction| {
                match direction {
                    LineDirection::Stderr => log::debug!("zeroclaw stderr: {line}"),
                    LineDirection::Stdout => log::trace!("zeroclaw acp rx: {line}"),
                    LineDirection::Stdin => log::trace!("zeroclaw acp tx: {line}"),
                }
            });

        let (conn_tx, conn_rx) = oneshot::channel();
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            builder
                .connect_with(agent, move |conn: ConnectionTo<Agent>| async move {
                    // handle 传出后挂住前台 future, 保持长连接直到收到关闭信号
                    let _ = conn_tx.send(conn);
                    let _ = shutdown_rx.await;
                    Ok(())
                })
                .await
        });

        match conn_rx.await {
            Ok(conn) => Ok(Self {
                conn,
                chunks,
                shutdown: Some(shutdown_tx),
                task: Some(task),
            }),
            Err(_) => {
                // 前台 future 没跑起来: 进程 spawn 失败或连接立刻断开
                match task.await {
                    Ok(Ok(())) => Err(anyhow!("zeroclaw acp 连接在建立前意外关闭")),
                    Ok(Err(e)) => Err(anyhow!("连接 zeroclaw acp 失败 ({}): {e}", bin.display())),
                    Err(e) => Err(anyhow!("zeroclaw 连接驱动 task 异常: {e}")),
                }
            }
        }
    }

    /// initialize 握手 (3s 超时)
    pub async fn initialize(&mut self) -> Result<()> {
        tokio::time::timeout(
            INITIALIZE_TIMEOUT,
            self.conn
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task(),
        )
        .await
        .context("zeroclaw initialize 超时")?
        .map_err(|e| anyhow!("zeroclaw initialize 失败: {e}"))?;
        Ok(())
    }

    /// 新建会话 (5s 超时)
    ///
    /// 返回的 session id 由调用方持有 (多 session: chat 长驻 + mood 临时).
    ///
    /// * `workspace` - agent 工作目录 (SOUL.md 所在, zeroclaw 作为 workspaceDir 返回)
    pub async fn session_new(&mut self, workspace: &Path) -> Result<String> {
        let resp = tokio::time::timeout(
            SESSION_TIMEOUT,
            self.conn
                .send_request(NewSessionRequest::new(workspace.to_path_buf()))
                .block_task(),
        )
        .await
        .context("zeroclaw session/new 超时")?
        .map_err(|e| anyhow!("zeroclaw session/new 失败: {e}"))?;
        Ok(resp.session_id.to_string())
    }

    /// 发送用户文本, 聚合流式分片直到本轮结束, 返回完整回复
    ///
    /// * `session_id` - `session_new` 返回的会话 id
    /// * `timeout` - 单轮整体超时 (由调用方按降级策略给定);
    ///   超时后 SDK 会向 agent 发 `$/cancel_request`, 本实例随后被调用方整体丢弃
    pub async fn prompt(
        &mut self,
        session_id: &str,
        text: &str,
        timeout: Duration,
    ) -> Result<String> {
        // 清掉上一轮残留分片, 本轮重新聚合
        self.chunks
            .lock()
            .expect("chunks 锁中毒")
            .remove(session_id);
        let req = PromptRequest::new(
            SessionId::new(session_id),
            vec![ContentBlock::Text(TextContent::new(text))],
        );
        tokio::time::timeout(timeout, self.conn.send_request(req).block_task())
            .await
            .context("等待 zeroclaw prompt 响应超时")?
            .map_err(|e| anyhow!("zeroclaw session/prompt 失败: {e}"))?;
        let reply = self
            .chunks
            .lock()
            .expect("chunks 锁中毒")
            .remove(session_id)
            .unwrap_or_default();
        Ok(reply)
    }

    /// 按 id 关闭会话 (清空记忆/优雅退出/临时 session 用完前调用);
    /// 内部发 SDK 的 `session/close` (zeroclaw 实测支持)
    pub async fn session_stop(&mut self, session_id: &str) -> Result<()> {
        tokio::time::timeout(
            SESSION_TIMEOUT,
            self.conn
                .send_request(CloseSessionRequest::new(SessionId::new(session_id)))
                .block_task(),
        )
        .await
        .context("zeroclaw session/close 超时")?
        .map_err(|e| anyhow!("zeroclaw session/close 失败: {e}"))?;
        Ok(())
    }

    /// 关闭连接并杀掉子进程 (进程组); 重建前显式清理.
    /// 触发前台 future 返回后, SDK 的关闭流程会给子进程 1s 退出宽限,
    /// 然后 SIGKILL 整个进程组 (Unix)
    pub async fn kill(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(task) = self.task.take() {
            if let Err(e) = tokio::time::timeout(KILL_TIMEOUT, task).await {
                // 超时不取消 task: 连接关闭流程仍在后台走完, 进程组终会被 SIGKILL
                log::warn!("等待 zeroclaw 连接关闭超时: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::{ContentChunk, PermissionOptionKind};

    fn chunk_notification(text: &str) -> SessionNotification {
        SessionNotification::new(
            SessionId::new("s"),
            SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
                TextContent::new(text),
            ))),
        )
    }

    #[test]
    fn extract_chunk_from_agent_message() {
        let n = chunk_notification("你好");
        assert_eq!(extract_chunk_text(&n), Some("你好"));
    }

    #[test]
    fn extract_chunk_ignores_other_updates() {
        // 思维链分片不算回复正文
        let thought = SessionNotification::new(
            SessionId::new("s"),
            SessionUpdate::AgentThoughtChunk(ContentChunk::new(ContentBlock::Text(
                TextContent::new("想..."),
            ))),
        );
        assert_eq!(extract_chunk_text(&thought), None);
    }

    fn option(id: &str, name: &str, kind: PermissionOptionKind) -> PermissionOption {
        PermissionOption::new(id.to_owned(), name.to_owned(), kind)
    }

    #[test]
    fn choose_first_allow_option() {
        let options = vec![
            option("reject-once", "Reject", PermissionOptionKind::RejectOnce),
            option("allow-once", "Allow once", PermissionOptionKind::AllowOnce),
            option(
                "allow-always",
                "Allow always",
                PermissionOptionKind::AllowAlways,
            ),
        ];
        let id = choose_allow_option(&options).unwrap();
        assert_eq!(id.0.as_ref(), "allow-once");
    }

    #[test]
    fn choose_allow_falls_back_to_name() {
        // id 不含 allow 时按名称匹配
        let options = vec![option(
            "opt-1",
            "Allow this time",
            PermissionOptionKind::AllowOnce,
        )];
        let id = choose_allow_option(&options).unwrap();
        assert_eq!(id.0.as_ref(), "opt-1");
    }

    #[test]
    fn choose_allow_none_when_no_allow() {
        let options = vec![option(
            "reject-once",
            "Reject",
            PermissionOptionKind::RejectOnce,
        )];
        assert!(choose_allow_option(&options).is_none());
        assert!(choose_allow_option(&[]).is_none());
    }
}
