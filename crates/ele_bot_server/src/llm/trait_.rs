//! LLM Trait 定义
//!
//! 统一在线和本地 LLM 的接口

use crate::llm::response::LlmResponse;
use anyhow::Result;
use async_trait::async_trait;

/// LLM Trait - 统一接口
#[allow(dead_code)]
#[async_trait]
pub trait LlmTrait: Send {
    /// 分析用户输入的情感
    async fn analyze_mood(&mut self, user_input: &str) -> Result<LlmResponse>;

    /// 生成对用户输入的对话文本回复 (走 TTS 播报).
    ///
    /// 默认实现返回 "LLM chat not implemented" 字符串, 方便 trait
    /// 的占位实现和测试用; 实际 `QwenLlm` / `OnlineLlm` 都 MUST 覆盖.
    async fn chat(&mut self, _user_input: &str) -> Result<String> {
        Ok("[LLM chat not implemented]".to_string())
    }

    /// 设置当前会话 ID
    fn set_session_id(&mut self, _session_id: &str) {}

    /// 清除指定会话的历史记录
    fn clear_session_history(&mut self, _session_id: &str) {}

    /// 清除所有会话的历史记录
    fn clear_all_histories(&mut self) {}
}
