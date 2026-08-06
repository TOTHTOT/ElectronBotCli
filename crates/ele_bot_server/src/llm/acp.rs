//! ACP (Agent Client Protocol) 客户端
//!
//! spawn `zeroclaw acp` 子进程, 通过 stdio 以 NDJSON (每行一个 JSON 对象)
//! 走 JSON-RPC 2.0. 协议细节见 `specs/001-zeroclaw-llm-integration/contracts/zeroclaw-acp.md`.
//!
//! # 状态机
//! `New` --`initialize`--> `Initialized` --`session/new`--> `Ready` (可 `prompt`).
//! 任何 IO 错误直接向上返回 `Err`, 由调用方 (`ZeroclawLlm`) 丢弃本实例整体重建.
//!
//! # 工具审批
//! ACP 模式下 zeroclaw 把工具审批委托给客户端 (`session/request_permission`,
//! server->client request); 本客户端一律回 allow-once (不在 zeroclaw 侧固化
//! 永久规则) — 可用工具与风险拦截由用户自己的 zeroclaw 配置/risk_profile 决定.

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

/// initialize 握手超时; zeroclaw 本地进程, 3s 足够
const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(3);
/// session/new 超时
const SESSION_NEW_TIMEOUT: Duration = Duration::from_secs(5);

/// ACP 客户端状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcpState {
    /// 子进程已 spawn, 未握手
    New,
    /// initialize 完成
    Initialized,
    /// session/new 完成, 可发起 prompt
    Ready,
}

/// 从子进程 stdout 读到的一行 JSON-RPC 消息的分类
#[derive(Debug, PartialEq, Eq)]
enum WireMessage {
    /// 对某个请求的响应 (含 result 或 error)
    Response(u64),
    /// `session/update` 通知里的助手文本分片
    AgentChunk,
    /// `session/request_permission` 工具审批请求 (server->client request,
    /// 必须回响应, 否则 agent 轮次挂起)
    PermissionRequest,
    /// 其它通知/请求, 忽略
    Other,
}

/// 解析一行 NDJSON, 返回分类.
///
/// 纯函数便于单测; 行格式以 zeroclaw acp v0.8.3 实测为准.
fn classify_line(line: &str) -> WireMessage {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return WireMessage::Other;
    };
    if v.get("id").and_then(Value::as_u64).is_some()
        && (v.get("result").is_some() || v.get("error").is_some())
    {
        return WireMessage::Response(v["id"].as_u64().unwrap_or(0));
    }
    match v.get("method").and_then(Value::as_str) {
        Some("session/update")
            if v["params"]["update"]["sessionUpdate"].as_str() == Some("agent_message_chunk") =>
        {
            WireMessage::AgentChunk
        }
        Some("session/request_permission") => WireMessage::PermissionRequest,
        _ => WireMessage::Other,
    }
}

/// 从 `session/request_permission` 行提取 (请求 id, 工具名).
///
/// id 原样保留 (实测是字符串 "zc-out-N", 回响应要原样带回);
/// 工具名取 `params.toolCall.rawInput.tool`, 兜底从 title "Approve X?" 解析
fn extract_permission(line: &str) -> Option<(Value, String)> {
    let v = serde_json::from_str::<Value>(line).ok()?;
    if classify_line(line) != WireMessage::PermissionRequest {
        return None;
    }
    let id = v.get("id")?.clone();
    let tool = v["params"]["toolCall"]["rawInput"]["tool"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| {
            v["params"]["toolCall"]["title"]
                .as_str()
                .and_then(|t| t.strip_prefix("Approve "))
                .and_then(|t| t.strip_suffix('?'))
                .map(str::to_owned)
        })
        .unwrap_or_default();
    Some((id, tool))
}

/// 审批策略: 全部 allow-once — zeroclaw 配置 (含可用工具与 risk_profile)
/// 完全由用户自管理, 本客户端不越权否决用户配置的工具;
/// 高危命令由 zeroclaw 侧 risk_profile (如 balanced 的 block_high_risk_commands) 拦截.
/// 不用 allow-always: 不在 zeroclaw 侧固化永久放行规则
fn approve_option(_tool: &str) -> &'static str {
    "allow-once"
}

/// 从 `session/update` 通知行提取文本分片; 非分片返回 None
fn extract_chunk(line: &str) -> Option<String> {
    let v = serde_json::from_str::<Value>(line).ok()?;
    if classify_line(line) != WireMessage::AgentChunk {
        return None;
    }
    v["params"]["update"]["content"]["text"]
        .as_str()
        .map(str::to_owned)
}

/// 从响应行提取 `result` 或把 `error` 转成 anyhow 错误
fn unwrap_response(line: &str) -> Result<Value> {
    let v: Value = serde_json::from_str(line).context("解析 JSON-RPC 响应失败")?;
    if let Some(err) = v.get("error") {
        let msg = err["message"].as_str().unwrap_or("未知错误");
        bail!("zeroclaw 返回错误: {msg}");
    }
    Ok(v["result"].clone())
}

/// ACP 客户端: 持有一个 `zeroclaw acp` 子进程
pub struct AcpClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    state: AcpState,
    /// session/new 成功后保存的 session id
    session_id: Option<String>,
}

impl AcpClient {
    /// spawn `zeroclaw acp` 子进程 (状态: New)
    ///
    /// 不传 `--config-dir`: zeroclaw 配置 (provider/api_key/人设) 完全由用户
    /// 自己维护 (默认 `~/.zeroclaw`), 本进程不介入渲染
    ///
    /// # Arguments
    ///
    /// * `bin` - zeroclaw 二进制路径
    pub fn spawn(bin: &Path) -> Result<Self> {
        let mut child = Command::new(bin)
            .arg("acp")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            // zeroclaw 的诊断日志走 stderr, 继承到本进程日志
            .stderr(std::process::Stdio::inherit())
            // 防止 client 实例被 drop 后孤儿进程常驻 (Broken 重建/服务退出)
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("spawn zeroclaw 失败: {}", bin.display()))?;
        let stdin = child.stdin.take().context("zeroclaw stdin 不可用")?;
        let stdout = child.stdout.take().context("zeroclaw stdout 不可用")?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 0,
            state: AcpState::New,
            session_id: None,
        })
    }

    /// 当前状态
    pub fn state(&self) -> AcpState {
        self.state
    }

    /// 当前 session id (Ready 状态才有)
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// 发送一个 JSON-RPC 请求, 返回分配的 id
    async fn send_request(&mut self, method: &str, params: Value) -> Result<u64> {
        self.next_id += 1;
        let id = self.next_id;
        let req = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let mut line = serde_json::to_string(&req)?;
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .await
            .context("向 zeroclaw 写请求失败")?;
        self.stdin
            .flush()
            .await
            .context("flush zeroclaw stdin 失败")?;
        Ok(id)
    }

    /// 读一行 stdout; EOF 视为子进程已退出
    async fn read_line(&mut self) -> Result<String> {
        let mut buf = String::new();
        let n = self
            .stdout
            .read_line(&mut buf)
            .await
            .context("读取 zeroclaw 响应失败")?;
        if n == 0 {
            bail!("zeroclaw 子进程已退出 (stdout EOF)");
        }
        Ok(buf)
    }

    /// 回应 `session/request_permission`: 按 `approve_option` 白名单策略选择,
    /// 回 JSON-RPC 响应 (id 原样带回); 不回应会导致 agent 轮次永久挂起
    async fn respond_permission(&mut self, line: &str) -> Result<()> {
        let Some((id, tool)) = extract_permission(line) else {
            return Ok(());
        };
        let option = approve_option(&tool);
        log::debug!("zeroclaw 工具审批: {tool} -> {option}");
        let resp = json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "outcome": { "outcome": "selected", "optionId": option } },
        });
        let mut buf = serde_json::to_string(&resp)?;
        buf.push('\n');
        self.stdin
            .write_all(buf.as_bytes())
            .await
            .context("回应 zeroclaw 审批请求失败")?;
        self.stdin.flush().await.context("flush 审批响应失败")?;
        Ok(())
    }

    /// 读响应直到匹配 `id`; 期间的分片通知喂给 `on_chunk`
    async fn wait_response(
        &mut self,
        id: u64,
        timeout: Duration,
        mut on_chunk: impl FnMut(&str),
    ) -> Result<Value> {
        tokio::time::timeout(timeout, async {
            loop {
                let line = self.read_line().await?;
                match classify_line(&line) {
                    WireMessage::Response(rid) if rid == id => {
                        return unwrap_response(&line);
                    }
                    WireMessage::AgentChunk => {
                        if let Some(text) = extract_chunk(&line) {
                            on_chunk(&text);
                        }
                    }
                    WireMessage::PermissionRequest => {
                        self.respond_permission(&line).await?;
                    }
                    _ => {}
                }
            }
        })
        .await
        .context("等待 zeroclaw 响应超时")?
    }

    /// initialize 握手: New -> Initialized
    pub async fn initialize(&mut self) -> Result<()> {
        let id = self
            .send_request(
                "initialize",
                json!({ "protocolVersion": 1, "clientCapabilities": {} }),
            )
            .await?;
        self.wait_response(id, INITIALIZE_TIMEOUT, |_| {}).await?;
        self.state = AcpState::Initialized;
        Ok(())
    }

    /// 新建会话: Initialized -> Ready
    ///
    /// * `workspace` - agent 工作目录 (SOUL.md 所在, zeroclaw 作为 workspaceDir 返回)
    pub async fn session_new(&mut self, workspace: &Path) -> Result<String> {
        let id = self
            .send_request("session/new", json!({ "cwd": workspace, "mcpServers": [] }))
            .await?;
        let result = self.wait_response(id, SESSION_NEW_TIMEOUT, |_| {}).await?;
        let session_id = result["sessionId"]
            .as_str()
            .context("session/new 响应缺少 sessionId")?
            .to_owned();
        self.session_id = Some(session_id.clone());
        self.state = AcpState::Ready;
        Ok(session_id)
    }

    /// 发送用户文本, 聚合流式分片直到本轮结束, 返回完整回复
    ///
    /// * `timeout` - 单轮整体超时 (由调用方按降级策略给定)
    pub async fn prompt(&mut self, text: &str, timeout: Duration) -> Result<String> {
        let session_id = self
            .session_id
            .clone()
            .context("session 未建立, 先调 session_new")?;
        let id = self
            .send_request(
                "session/prompt",
                json!({
                    "sessionId": session_id,
                    "prompt": [{ "type": "text", "text": text }],
                }),
            )
            .await?;
        let mut reply = String::new();
        self.wait_response(id, timeout, |chunk| reply.push_str(chunk))
            .await?;
        Ok(reply)
    }

    /// 停止当前会话 (清空记忆/优雅退出前调用); 无 session 时为 no-op
    pub async fn session_stop(&mut self) -> Result<()> {
        let Some(session_id) = self.session_id.take() else {
            return Ok(());
        };
        let id = self
            .send_request("session/stop", json!({ "sessionId": session_id }))
            .await?;
        self.wait_response(id, SESSION_NEW_TIMEOUT, |_| {}).await?;
        self.state = AcpState::Initialized;
        Ok(())
    }

    /// 杀掉子进程; 重建前显式清理, 避免孤儿进程
    pub async fn kill(&mut self) {
        let _ = self.child.kill().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_response_result() {
        let line = r#"{"jsonrpc":"2.0","result":{"sessionId":"abc"},"id":2}"#;
        assert_eq!(classify_line(line), WireMessage::Response(2));
    }

    #[test]
    fn classify_response_error() {
        let line = r#"{"jsonrpc":"2.0","error":{"code":-32603,"message":"boom"},"id":3}"#;
        assert_eq!(classify_line(line), WireMessage::Response(3));
    }

    #[test]
    fn classify_agent_chunk() {
        let line = r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"你好"}}}}"#;
        assert_eq!(classify_line(line), WireMessage::AgentChunk);
    }

    #[test]
    fn classify_others() {
        // 其它 notification
        let line = r#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"tool_call"}}}"#;
        assert_eq!(classify_line(line), WireMessage::Other);
        // 非 JSON 行
        assert_eq!(classify_line("not json"), WireMessage::Other);
    }

    #[test]
    fn extract_chunk_text() {
        let line = r#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"agent_message_chunk","content":{"text":"片段A"}}}}"#;
        assert_eq!(extract_chunk(line).as_deref(), Some("片段A"));
        // 非分片通知提取不到
        let other = r#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"sessionUpdate":"tool_call","content":{"text":"x"}}}}"#;
        assert_eq!(extract_chunk(other), None);
    }

    #[test]
    fn unwrap_response_result_and_error() {
        let ok = r#"{"jsonrpc":"2.0","result":{"a":1},"id":1}"#;
        assert_eq!(unwrap_response(ok).unwrap()["a"], json!(1));
        let err = r#"{"jsonrpc":"2.0","error":{"code":-1,"message":"账号欠费"},"id":1}"#;
        let msg = unwrap_response(err).unwrap_err().to_string();
        assert!(msg.contains("账号欠费"), "错误信息应透传: {msg}");
    }

    #[test]
    fn classify_permission_request() {
        let line = r#"{"jsonrpc":"2.0","method":"session/request_permission","params":{"options":[],"sessionId":"s","toolCall":{"rawInput":{"tool":"memory_store"},"title":"Approve memory_store?"}},"id":"zc-out-0"}"#;
        assert_eq!(classify_line(line), WireMessage::PermissionRequest);
    }

    #[test]
    fn extract_permission_id_and_tool() {
        let line = r#"{"jsonrpc":"2.0","method":"session/request_permission","params":{"toolCall":{"rawInput":{"tool":"glob_search"},"title":"Approve glob_search?"}},"id":"zc-out-3"}"#;
        let (id, tool) = extract_permission(line).unwrap();
        assert_eq!(id, json!("zc-out-3"));
        assert_eq!(tool, "glob_search");
        // rawInput.tool 缺失时从 title 兜底
        let fallback = r#"{"jsonrpc":"2.0","method":"session/request_permission","params":{"toolCall":{"title":"Approve file_edit?"}},"id":"zc-out-4"}"#;
        let (_, tool) = extract_permission(fallback).unwrap();
        assert_eq!(tool, "file_edit");
    }

    #[test]
    fn approve_option_allows_all() {
        // 工具审批全部 allow-once, 具体工具集由用户 zeroclaw 配置决定
        assert_eq!(approve_option("memory_store"), "allow-once");
        assert_eq!(approve_option("shell"), "allow-once");
        assert_eq!(approve_option(""), "allow-once");
    }
}
