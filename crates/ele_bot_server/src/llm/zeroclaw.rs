//! ZeroClaw LLM 适配器
//!
//! `chat()` 对话回复由设备端 zeroclaw 进程生成, 对话历史与用户记忆全部
//! 由 zeroclaw 侧的 SQLite 持有, 本结构体自身 **不保存任何对话历史**
//! (specs/001-zeroclaw-llm-integration: FR-002 的不变量).
//!
//! zeroclaw 的 provider / api_key / 人设 (SOUL.md) 等配置完全由用户自己
//! 维护 (默认 `~/.zeroclaw`), 本适配器只负责 spawn `zeroclaw acp` 并对话,
//! 不渲染、不下发任何 zeroclaw 配置.
//!
//! # 生命周期
//! - `new`: 仅记录二进制路径, 不 spawn 进程
//! - 首次 `chat`: 惰性 spawn `zeroclaw acp` + initialize + session/new
//! - 任何 ACP 错误: 丢弃子进程, 下次 `chat` 自动重建 (Broken 恢复)

use crate::llm::acp::AcpClient;
use anyhow::{bail, Context, Result};
use std::path::PathBuf;
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

/// ACP session 的工作目录: 优先 `ZCLAW_WORKSPACE`; 否则自动探测 zeroclaw 的
/// agent workspace (`agents/*/workspace`, 覆盖 `~/.zeroclaw` 与 homebrew
/// `/opt/homebrew/var/zeroclaw` 两种默认配置位置) — session cwd 与 agent
/// workspace 一致时, zeroclaw 才会把 SOUL.md/MEMORY.md 注入会话上下文
/// (实测: cwd 不匹配则人设与长期记忆都失效); 都找不到退回 `$HOME`
fn session_workspace() -> PathBuf {
    if let Ok(p) = std::env::var("ZCLAW_WORKSPACE") {
        return PathBuf::from(p);
    }
    let mut bases: Vec<PathBuf> = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        bases.push(PathBuf::from(home).join(".zeroclaw"));
    }
    bases.push(PathBuf::from("/opt/homebrew/var/zeroclaw"));
    for base in bases {
        let agents = base.join("agents");
        let Ok(entries) = std::fs::read_dir(&agents) else {
            continue;
        };
        for entry in entries.flatten() {
            let ws = entry.path().join("workspace");
            if ws.is_dir() {
                return ws;
            }
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
    /// None = 尚未建立或上次失败后已丢弃, 下次 chat 自动重建
    client: Option<AcpClient>,
}

impl Default for ZeroclawLlm {
    fn default() -> Self {
        Self::new()
    }
}

impl ZeroclawLlm {
    /// 创建适配器; 不 spawn 进程, 也不触碰 zeroclaw 配置
    pub fn new() -> Self {
        Self {
            bin: zeroclaw_bin(),
            workspace: session_workspace(),
            client: None,
        }
    }

    /// 确保 ACP 连接 Ready: 未建立则 spawn + 握手, 无 session 则补建
    async fn ensure_ready(&mut self) -> Result<()> {
        if self.client.is_none() {
            let mut client = AcpClient::spawn(&self.bin)?;
            if let Err(e) = client.initialize().await {
                client.kill().await;
                return Err(e.context("zeroclaw initialize 失败"));
            }
            self.client = Some(client);
        }
        let client = self.client.as_mut().expect("client just ensured");
        if client.session_id().is_none() {
            client
                .session_new(&self.workspace)
                .await
                .context("zeroclaw session/new 失败")?;
        }
        Ok(())
    }

    /// 出错后丢弃子进程, 下次 chat 整体重建 (spec: Broken 自动恢复)
    async fn drop_client(&mut self) {
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
        match client.prompt(user_input, PROMPT_TIMEOUT).await {
            Ok(reply) => Ok(reply),
            Err(e) => {
                self.drop_client().await;
                Err(e)
            }
        }
    }

    /// 清空全部对话历史与个人记忆 (spec: FR-006 整体清空入口)
    ///
    /// 流程: ACP session/stop → `zeroclaw memory clear --yes` → 下次 chat
    /// 自动 session/new 重建 (spec contracts/zeroclaw-config.md 命令链).
    pub async fn clear_memory(&mut self) -> Result<()> {
        if let Some(client) = self.client.as_mut() {
            client
                .session_stop()
                .await
                .context("zeroclaw session/stop 失败")?;
        }
        let status = Command::new(&self.bin)
            .args(["memory", "clear", "--yes"])
            .status()
            .await
            .context("执行 zeroclaw memory clear 失败")?;
        if !status.success() {
            bail!("zeroclaw memory clear 退出码: {status}");
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
