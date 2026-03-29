//! LLM Trait 定义
//!
//! 统一在线和本地 LLM 的接口

use crate::llm::response::LlmResponse;
use anyhow::Result;

/// LLM Trait - 统一接口
pub trait LlmTrait: Send {
    /// 分析用户输入的情感
    fn analyze_mood(&mut self, user_input: &str) -> Result<LlmResponse>;
}
