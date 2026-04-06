//! 在线 LLM 模块
//!
//! 使用 async-openai 调用在线 LLM API（如豆包/火山引擎）
//!
//! # 设计说明
//! - `OnlineLlm` 实现了 `Send + Sync`，可在多线程间共享
//! - 使用 `tokio::task::block_in_place` 在同步上下文中执行异步代码
//! - 请求默认超时 30 秒

use crate::emotion::parse_mood;
use crate::llm::response::LlmResponse;
use crate::llm::trait_::LlmTrait;
use anyhow::{Context, Result};
use async_openai::config::OpenAIConfig;
use async_openai::types::chat::{
    ChatCompletionRequestSystemMessageArgs, ChatCompletionRequestUserMessageArgs,
    CreateChatCompletionRequestArgs,
};
use async_openai::Client;

/// 在线 LLM 实现
///
/// 使用 async-openai 库的异步客户端，支持任意兼容 OpenAI API 格式的后端。
///
/// # 线程安全
/// 该结构体实现了 `Send + Sync`，可以在多线程间共享同一个实例。
pub struct OnlineLlm {
    /// 异步 OpenAI 客户端
    client: Client<OpenAIConfig>,
    /// 模型名称
    model: String,
}

impl OnlineLlm {
    /// 创建新的在线 LLM 实例
    ///
    /// # 参数
    /// - `api_base`: API 基础 URL（如 `https://ark.cn-beijing.volces.com/api/v3`）
    /// - `api_key`: API 密钥
    /// - `model`: 模型名称（如 `doubao-voice-2025-01-25`）
    pub fn new(api_base: &str, api_key: &str, model: &str) -> Result<Self> {
        if api_base.is_empty() {
            anyhow::bail!("API base URL cannot be empty");
        }
        if api_key.is_empty() {
            anyhow::bail!("API key cannot be empty");
        }

        let config = OpenAIConfig::new()
            .with_api_base(api_base)
            .with_api_key(api_key);

        let client = Client::with_config(config);

        Ok(Self {
            client,
            model: model.to_string(),
        })
    }

    /// 构建情感分析的 system prompt
    fn system_prompt() -> &'static str {
        // 明确要求输出格式与 parse_mood 的解析逻辑一致
        // parse_mood 使用 starts_with("[开心]") 等方式匹配
        "你是一个情感分析助手。请分析用户输入的情感，只输出情感标签。\n\n情感选项及输出格式：\n- 开心时请输出：[开心]\n- 难过时请输出：[难过]\n- 生气时请输出：[生气]\n- 惊讶时请输出：[惊讶]\n- 困惑时请输出：[困惑]\n- 其他或中性时输出：[中性]\n\n只输出情感标签，不要输出其他内容。"
    }

    /// 构建用户输入的 prompt
    fn build_user_prompt(&self, user_input: &str) -> String {
        format!("用户输入：{}", user_input)
    }

    /// 异步执行情感分析（内部实现）
    ///
    /// # 错误处理
    /// 返回的错误可能来自：
    /// - 网络连接问题
    /// - API 请求超时
    /// - API 返回错误（如认证失败、限流等）
    async fn analyze_mood_async(&self, user_input: &str) -> Result<LlmResponse> {
        let user_prompt = self.build_user_prompt(user_input);

        // 构建消息
        let system_msg = ChatCompletionRequestSystemMessageArgs::default()
            .content(Self::system_prompt())
            .build()?
            .into();

        let user_msg = ChatCompletionRequestUserMessageArgs::default()
            .content(user_prompt)
            .build()?
            .into();

        // 构建请求
        let request = CreateChatCompletionRequestArgs::default()
            .model(&self.model)
            .messages(vec![system_msg, user_msg])
            .temperature(0.0)
            .max_tokens(256u32)
            .build()?;

        log::debug!(
            "Online LLM request: {}",
            serde_json::to_string(&request).unwrap_or_default()
        );

        // 调用 API
        let response = self
            .client
            .chat()
            .create(request)
            .await
            .context("LLM API 调用失败，请检查网络连接和 API 配置")?;

        // 提取响应内容
        let content = response
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .unwrap_or_default();

        log::debug!("Online LLM response: {}", content);

        // 解析情感
        let mood = parse_mood(&content);

        Ok(LlmResponse {
            mood,
            actions: Vec::new(),
        })
    }
}

impl LlmTrait for OnlineLlm {
    fn analyze_mood(&mut self, user_input: &str) -> Result<LlmResponse> {
        // 由于调用方可能在非 async 上下文（如 std::thread）中调用，
        // 需要创建新的 Tokio runtime 来执行异步代码
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(self.analyze_mood_async(user_input))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_prompt_format() {
        let prompt = OnlineLlm::system_prompt();
        // 验证 prompt 中包含了正确的情感标签格式
        assert!(prompt.contains("[开心]"));
        assert!(prompt.contains("[难过]"));
        assert!(prompt.contains("[生气]"));
    }
}
