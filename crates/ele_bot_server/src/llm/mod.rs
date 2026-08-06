//! LLM 模块
//!
//! 双后端结构 (specs/001-zeroclaw-llm-integration):
//! - `chat` 对话回复: 由 zeroclaw 进程托管 (含对话历史与用户记忆)
//! - `analyze_mood` 情感/动作分析: 保留现有在线 LLM API 或本地 Candle GGUF 链路
//!
//! ## 使用方法
//!
//! ```ignore
//! let manager = LlmManager::new(api_base, api_key, model, model_path, tokenizer_path)?;
//! let reply = manager.chat("你好").await?;            // zeroclaw 托管历史
//! let response = manager.analyze_mood("你好").await?;  // 现有链路
//! ```
//!
//! ## 文件要求 (本地 GGUF 兜底, analyze_mood 用)
//!
//! - 模型: `your_path/Qwen3-0.6B-Q3_K_M.gguf`
//! - 分词器: `your_path/tokenizer.json`

pub mod acp;
pub mod online;
pub mod qwen;
pub mod response;
pub mod trait_;
pub mod zeroclaw;

use crate::llm::online::OnlineLlm;
use crate::llm::qwen::QwenLlm;
pub use crate::llm::response::LlmResponse;
use crate::llm::trait_::LlmTrait;
use crate::llm::zeroclaw::ZeroclawLlm;
use anyhow::{bail, Result};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

/// LLM 管理器
///
/// - `chat`: 走 zeroclaw (对话历史/记忆托管, zeroclaw 配置由用户自管理);
///   进程级故障在首次 chat 时快速失败, 由上层降级播报 (spec US3)
/// - `analyze_mood`: 根据网络状态自动选择在线或本地 LLM (spec Q2: 不迁移)
pub struct LlmManager {
    /// analyze_mood 链路: 在线/本地 LLM
    mood_llm: Arc<Mutex<Box<dyn LlmTrait>>>,
    /// chat 链路: zeroclaw (始终启用, 惰性连接; None 仅为防御性兜底)
    chat_llm: Option<Arc<Mutex<ZeroclawLlm>>>,
}

#[allow(dead_code)]
impl LlmManager {
    /// 创建 LLM 管理器
    ///
    /// # Arguments
    ///
    /// * `api_base` - 在线 LLM API 地址
    /// * `api_key` - 在线 LLM API Key
    /// * `model` - 在线 LLM 模型名称
    /// * `model_path` - 本地模型路径
    /// * `tokenizer_path` - 分词器路径
    ///
    /// # Returns
    ///
    /// mood 链路: 有网络且配置有效用在线 LLM, 否则本地 LLM (失败则整体报错);
    /// chat 链路: 始终启用, zeroclaw 进程故障在首次 chat 时快速失败降级
    pub fn new(
        api_base: &str,
        api_key: &str,
        model: &str,
        model_path: PathBuf,
        tokenizer_path: PathBuf,
    ) -> Result<Self> {
        // 检测网络状态
        let is_online = Self::check_network();

        log::info!("Network status: {is_online}, trying to create LLM");

        let mood_llm: Box<dyn LlmTrait> =
            if is_online && !api_base.is_empty() && !api_key.is_empty() {
                // 尝试创建在线 LLM
                match OnlineLlm::new(api_base, api_key, model) {
                    Ok(online) => {
                        log::info!("Using online LLM");
                        Box::new(online)
                    }
                    Err(e) => {
                        log::warn!("Failed to create online LLM: {e}, falling back to local");
                        Self::create_local_llm(model_path, tokenizer_path)?
                    }
                }
            } else {
                log::info!("Using local Qwen LLM");
                Self::create_local_llm(model_path, tokenizer_path)?
            };

        // chat 链路始终启用: zeroclaw 配置由用户自己维护 (默认 ~/.zeroclaw),
        // 进程级故障在首次 chat 时快速失败并降级播报 (spec US3)
        log::info!("zeroclaw chat 链路已配置 (首次对话时惰性启动)");
        let chat_llm = Some(Arc::new(Mutex::new(ZeroclawLlm::new())));

        Ok(Self {
            mood_llm: Arc::new(Mutex::new(mood_llm)),
            chat_llm,
        })
    }

    /// 检测网络状态
    fn check_network() -> bool {
        // 使用 std::net::TcpStream 检测网络
        std::net::TcpStream::connect_timeout(
            &std::net::SocketAddr::from(([8, 8, 8, 8], 53)),
            std::time::Duration::from_secs(3),
        )
        .is_ok()
    }

    /// 创建本地 LLM
    fn create_local_llm(model_path: PathBuf, tokenizer_path: PathBuf) -> Result<Box<dyn LlmTrait>> {
        let mut llm = QwenLlm::load(model_path);
        llm.load_tokenizer(tokenizer_path)?;
        llm.preload()?;
        Ok(Box::new(llm))
    }

    /// 分析情感
    pub async fn analyze_mood(&self, user_input: &str) -> Result<LlmResponse> {
        let mut guard = self.mood_llm.lock().await;
        guard.analyze_mood(user_input).await
    }

    /// 生成对话文本回复 (走 TTS 播报, zeroclaw 托管历史与记忆).
    /// 内部 tokio Mutex 借用 `&self.chat_llm`.
    pub async fn chat(&self, user_input: &str) -> Result<String> {
        log::debug!("user_input zeroclaw: {user_input}");
        let Some(zc) = &self.chat_llm else {
            bail!("zeroclaw 未启用 (llm 配置不完整或初始化失败)");
        };
        zc.lock().await.chat(user_input).await
    }

    /// 清空全部对话历史与个人记忆 (spec: FR-006)
    pub async fn clear_llm_memory(&self) -> Result<()> {
        let Some(zc) = &self.chat_llm else {
            bail!("zeroclaw 未启用, 无记忆可清空");
        };
        zc.lock().await.clear_memory().await
    }
}
