//! LLM 模块
//!
//! 单后端结构 (docs/superpowers/specs/2026-08-06-zeroclaw-mood-migration-design.md):
//! `chat` 对话回复与 `analyze_mood` 情感/动作分析全部由 zeroclaw 进程托管
//! (对话历史与用户记忆在 zeroclaw 侧, 配置由用户自管理)
//!
//! ## 使用方法
//!
//! ```ignore
//! let manager = LlmManager::new();
//! let reply = manager.chat("你好").await?;            // zeroclaw chat session
//! let response = manager.analyze_mood("你好").await?; // zeroclaw 临时 mood session
//! ```

pub mod acp;
pub mod response;
pub mod zeroclaw;

pub use crate::llm::response::LlmResponse;
use crate::llm::zeroclaw::ZeroclawLlm;
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Mutex;

/// LLM 管理器
///
/// 单一后端 zeroclaw (始终启用, 惰性连接):
/// - `chat`: 长驻 session, 对话历史/记忆由 zeroclaw 托管;
///   进程级故障在首次 chat 时快速失败, 由上层降级播报 (spec US3)
/// - `analyze_mood`: 每次分析新建临时 mood session, 失败回退中性
///   (上层 state.rs 已有 `LlmResponse::default()` 兜底)
pub struct LlmManager {
    llm: Arc<Mutex<ZeroclawLlm>>,
}

impl LlmManager {
    /// 创建 LLM 管理器; 不 spawn 进程, zeroclaw 连接在首次调用时惰性建立
    #[must_use]
    pub fn new() -> Self {
        Self {
            llm: Arc::new(Mutex::new(ZeroclawLlm::new())),
        }
    }

    /// 分析情感 (zeroclaw 临时 mood session)
    pub async fn analyze_mood(&self, user_input: &str) -> Result<LlmResponse> {
        self.llm.lock().await.analyze_mood(user_input).await
    }

    /// 生成对话文本回复 (走 TTS 播报, zeroclaw 托管历史与记忆).
    /// 内部 tokio Mutex 借用 `&self.llm`.
    pub async fn chat(&self, user_input: &str) -> Result<String> {
        log::debug!("user_input zeroclaw: {user_input}");
        self.llm.lock().await.chat(user_input).await
    }

    /// 清空全部对话历史与个人记忆 (spec: FR-006); 只清 chat 侧
    pub async fn clear_llm_memory(&self) -> Result<()> {
        self.llm.lock().await.clear_memory().await
    }
}

impl Default for LlmManager {
    fn default() -> Self {
        Self::new()
    }
}
