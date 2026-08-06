//! ZeroClaw LLM 适配器
//!
//! `chat()` 对话回复与 `analyze_mood()` 情感/动作分析都由设备端 zeroclaw
//! 进程生成, 对话历史与用户记忆全部由 zeroclaw 侧的 SQLite 持有,
//! 本结构体自身 **不保存任何对话历史**
//! (specs/001-zeroclaw-llm-integration: FR-002 的不变量).
//!
//! - chat: 长驻 session, 历史/记忆滚雪球是特性
//! - mood: 每次分析新建临时 session (`session/new` → `prompt` → `session/close`),
//!   保证每轮分析都是干净上下文, 不污染 chat 侧对话历史
//!
//! zeroclaw 的 provider / api_key / 人设 (SOUL.md) 等配置完全由用户自己
//! 维护 (默认 `~/.zeroclaw`), 本适配器只负责 spawn `zeroclaw acp` 并对话,
//! 不渲染、不下发任何 zeroclaw 配置.
//!
//! # 生命周期
//! - `new`: 仅记录二进制路径, 不 spawn 进程
//! - 首次 `chat`/`analyze_mood`: 惰性 spawn `zeroclaw acp` + initialize + session/new
//! - 任何 ACP 错误: 丢弃子进程, 下次调用自动重建 (Broken 恢复)

use crate::llm::acp::AcpClient;
use crate::llm::response::{parse_actions, parse_mood, split_response, system_prompt, LlmResponse};
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;

/// 单轮 prompt 超时兜底, 防止 zeroclaw 挂死时永久阻塞.
/// 注意要覆盖慢轮次: 记忆写入类轮次实测 40s+ (多次工具调用),
/// 普通轮次经代理 4-15s; 进程级故障 (spawn/initialize) 走快速失败路径,
/// 不依赖这个值 (spec US3 的 5s 反馈由 state.rs 的快失败分支保证)
const PROMPT_TIMEOUT: Duration = Duration::from_secs(90);

/// 部署目录: zeroclaw 与 ele_bot_server 同目录下发
/// (见 scripts/deploy_rk3566.sh); 取不到可执行文件路径时退回当前目录
fn deploy_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// zeroclaw 二进制路径 (默认同部署目录; 可用 ZCLAW_BIN 覆盖, 便于 macOS 联调)
fn zeroclaw_bin() -> PathBuf {
    std::env::var("ZCLAW_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|_| deploy_dir().join("zeroclaw"))
}

/// 找 `<base>/agents/*/workspace` 下第一个存在的 workspace
fn first_agent_workspace(base: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(base.join("agents")).ok()?;
    for entry in entries.flatten() {
        let ws = entry.path().join("workspace");
        if ws.is_dir() {
            return Some(ws);
        }
    }
    None
}

/// 通过 `zeroclaw status` 解析真实配置目录下的 agent workspace.
/// status 与 acp 子进程走同一套配置解析, 比路径猜测可靠:
/// homebrew 默认 `/opt/homebrew/var/zeroclaw`, 手动安装默认 `~/.zeroclaw`,
/// 两者可能同时存在, 猜错会把记忆清到别的 agent 头上
fn resolve_agent_workspace(bin: &Path) -> Option<PathBuf> {
    let output = std::process::Command::new(bin)
        .arg("status")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let config_path = stdout
        .lines()
        .find_map(|l| l.trim().strip_prefix("Config:"))?;
    let config_dir = PathBuf::from(config_path.trim()).parent()?.to_path_buf();
    first_agent_workspace(&config_dir)
}

/// ACP session 的工作目录 (也是 `clear_memory` 清记忆文件的作用范围):
/// 优先 `ZCLAW_WORKSPACE`; 否则用 `zeroclaw status` 解析实际生效的 agent
/// workspace; 解析失败退回路径探测 (覆盖 `~/.zeroclaw` 与 homebrew
/// `/opt/homebrew/var/zeroclaw` 两种默认配置位置); 都找不到退回 `$HOME`.
/// 注意: MEMORY.md 等上下文注入跟随 zeroclaw 配置的默认 agent, 与 cwd 无关,
/// 但 cwd 与 agent workspace 一致能保证 file 工具读写的是同一份文件
fn session_workspace(bin: &Path) -> PathBuf {
    if let Ok(p) = std::env::var("ZCLAW_WORKSPACE") {
        return PathBuf::from(p);
    }
    if let Some(ws) = resolve_agent_workspace(bin) {
        return ws;
    }
    let mut bases: Vec<PathBuf> = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        bases.push(PathBuf::from(home).join(".zeroclaw"));
    }
    bases.push(PathBuf::from("/opt/homebrew/var/zeroclaw"));
    for base in &bases {
        if let Some(ws) = first_agent_workspace(base) {
            return ws;
        }
    }
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/"))
}

/// ZeroClaw 对话适配器: 持有惰性建立的 ACP 连接
pub struct ZeroclawLlm {
    bin: PathBuf,
    workspace: PathBuf,
    /// None = 尚未建立或上次失败后已丢弃, 下次调用自动重建
    client: Option<AcpClient>,
    /// chat 长驻 session id (mood 用临时 session, 不记在这里)
    chat_session: Option<String>,
}

impl Default for ZeroclawLlm {
    fn default() -> Self {
        Self::new()
    }
}

impl ZeroclawLlm {
    /// 创建适配器; 不 spawn 进程, 也不触碰 zeroclaw 配置
    pub fn new() -> Self {
        let bin = zeroclaw_bin();
        Self {
            workspace: session_workspace(&bin),
            bin,
            client: None,
            chat_session: None,
        }
    }

    /// 确保 ACP 连接已建立: 未建立则 spawn + 握手, 无 chat session 则补建
    async fn ensure_ready(&mut self) -> Result<()> {
        if self.client.is_none() {
            let mut client = AcpClient::spawn(&self.bin).await?;
            if let Err(e) = client.initialize().await {
                client.kill().await;
                return Err(e.context("zeroclaw initialize 失败"));
            }
            self.client = Some(client);
        }
        if self.chat_session.is_none() {
            let client = self.client.as_mut().expect("client just ensured");
            let session_id = client
                .session_new(&self.workspace)
                .await
                .context("zeroclaw session/new 失败")?;
            self.chat_session = Some(session_id);
        }
        Ok(())
    }

    /// 出错后丢弃子进程, 下次调用整体重建 (spec: Broken 自动恢复)
    async fn drop_client(&mut self) {
        self.chat_session = None;
        if let Some(mut client) = self.client.take() {
            client.kill().await;
        }
    }

    /// 生成对话文本回复 (走 TTS 播报); 历史/记忆由 zeroclaw 托管
    ///
    /// prompt 失败自动重建连接重试一次: 上层 (state.rs) 的播报超时比
    /// `PROMPT_TIMEOUT` 短, 会提前取消 prompt future, 把 session 留在
    /// "active prompt turn" 的楔死状态; 杀掉子进程整体重建即可恢复,
    /// 对调用方透明 (代价是 session 内短期上下文重置, 长期记忆不受影响)
    pub async fn chat(&mut self, user_input: &str) -> Result<String> {
        match self.prompt_once(user_input).await {
            Ok(reply) => Ok(reply),
            Err(e) => {
                log::warn!("zeroclaw prompt 失败, 重建连接重试一次: {e:?}");
                self.drop_client().await;
                self.prompt_once(user_input).await
            }
        }
    }

    /// 单轮 prompt: 确保连接 Ready 后发起, 出错即丢弃子进程
    async fn prompt_once(&mut self, user_input: &str) -> Result<String> {
        self.ensure_ready().await?;
        let client = self.client.as_mut().expect("client ready");
        let session_id = self.chat_session.clone().expect("chat session ready");
        match client.prompt(&session_id, user_input, PROMPT_TIMEOUT).await {
            Ok(reply) => Ok(reply),
            Err(e) => {
                self.drop_client().await;
                Err(e)
            }
        }
    }

    /// 情感/动作分析: 每次新建临时 mood session, 用完即停, 不污染
    /// chat 侧对话历史与长期记忆; 任何一步出错丢弃子进程整体重建
    /// (与 `prompt_once` 一致, 不重试 — 上层 state.rs 已有中性兜底)
    ///
    /// prompt 为自包含单条消息: `system_prompt()` 指令文本 + 用户输入;
    /// 解析失败 (模型没按格式输出) 由 `split_response`/`parse_mood` 兜底
    /// 为 `[中性]` + 空动作, 不报错
    pub async fn analyze_mood(&mut self, user_input: &str) -> Result<LlmResponse> {
        let result = self.analyze_mood_once(user_input).await;
        if result.is_err() {
            self.drop_client().await;
        }
        result
    }

    /// `analyze_mood` 的单次尝试: session/new → prompt → session/close → 解析
    async fn analyze_mood_once(&mut self, user_input: &str) -> Result<LlmResponse> {
        self.ensure_ready().await?;
        let client = self.client.as_mut().expect("client ready");
        let mood_session = client
            .session_new(&self.workspace)
            .await
            .context("zeroclaw mood session/new 失败")?;
        let text = format!("{}\n\n用户输入：{user_input}", system_prompt());
        let prompt_result = client.prompt(&mood_session, &text, PROMPT_TIMEOUT).await;
        // session 已用完, close 失败只告警不中断 (进程整体重建时会清掉)
        if let Err(e) = client.session_stop(&mood_session).await {
            log::warn!("zeroclaw mood session/close 失败: {e:?}");
        }
        let reply = prompt_result?;
        log::info!("zeroclaw mood response: {reply}");

        let (mood_str, actions_str) = split_response(&reply);
        let mood = parse_mood(mood_str);
        let actions = parse_actions(actions_str);
        log::info!("Mood: {mood:?}, Actions count: {}", actions.len());
        Ok(LlmResponse { mood, actions })
    }

    /// 清空全部对话历史与个人记忆 (spec: FR-006 整体清空入口)
    ///
    /// 流程: ACP session/close → `zeroclaw memory clear --yes` (SQLite 会话
    /// 历史与记忆条目) → 删除 workspace 的 `MEMORY.md` (agent 自己写入的
    /// 长期记忆, 每 session 注入 system prompt) 和 `memory/` 目录 (daily
    /// 原始记录, 若存在) — 后两者不在 SQLite 里, 只清 CLI 会残留个人信息.
    /// 下次 chat 自动 session/new 重建 (spec contracts/zeroclaw-config.md).
    /// **不动** USER.md / SOUL.md / AGENTS.md 等用户自管配置文件.
    /// SQLite 已清即算主体成功, 后续文件清理失败只告警不中断.
    pub async fn clear_memory(&mut self) -> Result<()> {
        if let Some(client) = self.client.as_mut() {
            if let Some(session_id) = self.chat_session.take() {
                client
                    .session_stop(&session_id)
                    .await
                    .context("zeroclaw session/close 失败")?;
            }
        }
        let status = Command::new(&self.bin)
            .args(["memory", "clear", "--yes"])
            .status()
            .await
            .context("执行 zeroclaw memory clear 失败")?;
        if !status.success() {
            bail!("zeroclaw memory clear 退出码: {status}");
        }
        // 清 workspace 里 agent 自己写入的记忆文件/目录 (不在 SQLite 覆盖范围)
        let memory_md = self.workspace.join("MEMORY.md");
        if let Err(e) = tokio::fs::remove_file(&memory_md).await {
            if e.kind() != std::io::ErrorKind::NotFound {
                log::warn!("删除 {} 失败: {e}", memory_md.display());
            }
        }
        let memory_dir = self.workspace.join("memory");
        if let Err(e) = tokio::fs::remove_dir_all(&memory_dir).await {
            if e.kind() != std::io::ErrorKind::NotFound {
                log::warn!("删除 {} 失败: {e}", memory_dir.display());
            }
        }
        log::info!("zeroclaw 对话历史与记忆已清空");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bin_defaults_next_to_exe() {
        let bin = zeroclaw_bin();
        assert_eq!(bin.file_name().and_then(|n| n.to_str()), Some("zeroclaw"));
    }

    #[test]
    fn new_does_not_spawn() {
        let zc = ZeroclawLlm::new();
        assert!(zc.client.is_none());
    }
}
