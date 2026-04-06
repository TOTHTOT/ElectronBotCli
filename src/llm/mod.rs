//! LLM 模块
//!
//! 使用 Candle 加载 GGUF 模型进行推理，或使用在线 LLM API
//!
//! ## 使用方法
//!
//! ```rust
//! use ele_bot::llm::{QwenLlm, OnlineLlm, LlmManager};
//!
//! // 本地模型加载
//! let mut llm = QwenLlm::load("your_path/qwen3-0.6b-rust-sft-q8_0.gguf")?;
//! llm.load_tokenizer("your_path/tokenizer.json")?;
//! let response = llm.analyze_mood("你好")?;
//! println!("Mood: {:?}, Actions: {:?}", response.mood, response.actions);
//! ```
//!
//! ## 文件要求
//!
//! - 模型: `your_path/Qwen3-0.6B-Q3_K_M.gguf`
//! - 分词器: `your_path/tokenizer.json`

pub mod online;
pub mod qwen;
pub mod response;
pub mod trait_;

use crate::llm::online::OnlineLlm;
use crate::llm::qwen::QwenLlm;
pub use crate::llm::response::LlmResponse;
use crate::llm::trait_::LlmTrait;
use anyhow::Result;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// LLM 管理器 - 根据网络状态自动选择在线或本地 LLM
pub struct LlmManager {
    inner: Arc<Mutex<Box<dyn LlmTrait>>>,
}

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
    /// 如果有网络且配置有效，返回在线 LLM；否则返回本地 LLM
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

        let inner: Box<dyn LlmTrait> = if is_online && !api_base.is_empty() && !api_key.is_empty() {
            // 尝试创建在线 LLM
            match OnlineLlm::new(api_base, api_key, model) {
                Ok(online) => {
                    log::info!("Using online LLM");
                    Box::new(online)
                }
                Err(e) => {
                    log::warn!("Failed to create online LLM: {}, falling back to local", e);
                    Self::create_local_llm(model_path, tokenizer_path)?
                }
            }
        } else {
            log::info!("Using local Qwen LLM");
            Self::create_local_llm(model_path, tokenizer_path)?
        };

        Ok(Self {
            inner: Arc::new(Mutex::new(inner)),
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
    pub fn analyze_mood(&self, user_input: &str) -> Result<LlmResponse> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        guard.analyze_mood(user_input)
    }
}

// 为 QwenLlm 实现 LlmTrait
impl LlmTrait for QwenLlm {
    fn analyze_mood(&mut self, user_input: &str) -> Result<LlmResponse> {
        let mood = QwenLlm::analyze_mood(self, user_input)?;
        Ok(LlmResponse {
            mood,
            actions: Vec::new(),
        })
    }
}
