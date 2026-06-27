//! LLM Trait 定义
//!
//! 统一在线和本地 LLM 的接口

use crate::llm::response::LlmResponse;
use anyhow::Result;

/// LLM Trait - 统一接口
#[allow(dead_code)]
pub trait LlmTrait: Send {
    /// 分析用户输入的情感
    fn analyze_mood(&mut self, user_input: &str) -> Result<LlmResponse>;

    /// 设置当前会话 ID
    fn set_session_id(&mut self, _session_id: &str) {}

    /// 清除指定会话的历史记录
    fn clear_session_history(&mut self, _session_id: &str) {}

    /// 清除所有会话的历史记录
    fn clear_all_histories(&mut self) {}
}
