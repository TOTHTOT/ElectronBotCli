//! 在线 LLM 模块
//!
//! 使用 async-openai 调用在线 LLM API（如豆包/火山引擎）

use crate::emotion::parse_mood;
use crate::llm::response::LlmResponse;
use crate::llm::trait_::LlmTrait;
use anyhow::{Context, Result};
use async_openai::config::OpenAIConfig;
use async_openai::types::chat::{
    ChatCompletionRequestUserMessageArgs, CreateChatCompletionRequestArgs,
};
use async_openai::Client;
use std::sync::Mutex;

/// 在线 LLM 实现
pub struct OnlineLlm {
    client: Client<OpenAIConfig>,
    model: String,
}

impl OnlineLlm {
    /// 创建新的在线 LLM 实例
    pub fn new(api_base: &str, api_key: &str, model: &str) -> Result<Self> {
        if api_base.is_empty() || api_key.is_empty() {
            anyhow::bail!("API base or key is empty");
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

    /// 构建情感分析 prompt
    fn build_prompt(&self, user_input: &str) -> String {
        format!(
            "分析用户输入的情感，只输出情感标签。\n情感选项：开心、难过、生气、困惑、害怕、中性\n输出格式：[情感]\n用户输入：{}",
            user_input
        )
    }

    /// 异步执行情感分析
    async fn analyze_mood_async(&self, user_input: &str) -> Result<LlmResponse> {
        let prompt = self.build_prompt(user_input);

        // 使用 builder 模式构建请求
        let user_msg = ChatCompletionRequestUserMessageArgs::default()
            .content(prompt.clone())
            .build()?
            .into();

        let system_msg =
            async_openai::types::chat::ChatCompletionRequestSystemMessageArgs::default()
                .content("你是一个情感分析助手，只输出情感标签")
                .build()?
                .into();

        let request = CreateChatCompletionRequestArgs::default()
            .model(&self.model)
            .messages(vec![system_msg, user_msg])
            .temperature(0.0)
            .build()?;

        log::info!(
            "Online LLM request: {}",
            serde_json::to_string(&request).unwrap_or_default()
        );

        let response = self
            .client
            .chat()
            .create(request)
            .await
            .context("Failed to create chat completion")?;

        let content = response
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .unwrap_or_default();

        log::info!("Online LLM response: {}", content);

        let mood = parse_mood(&content);

        Ok(LlmResponse {
            mood,
            actions: Vec::new(),
        })
    }
}

impl LlmTrait for OnlineLlm {
    fn analyze_mood(&mut self, user_input: &str) -> Result<LlmResponse> {
        // 使用 tokio runtime 执行异步调用
        let runtime = tokio::runtime::Runtime::new().context("Failed to create tokio runtime")?;
        runtime.block_on(self.analyze_mood_async(user_input))
    }
}

/// 线程安全的在线 LLM 包装器
pub struct OnlineLlmWrapper {
    inner: Mutex<OnlineLlm>,
}

impl OnlineLlmWrapper {
    pub fn new(api_base: &str, api_key: &str, model: &str) -> Result<Self> {
        let inner = OnlineLlm::new(api_base, api_key, model)?;
        Ok(Self {
            inner: Mutex::new(inner),
        })
    }
}

impl LlmTrait for OnlineLlmWrapper {
    fn analyze_mood(&mut self, user_input: &str) -> Result<LlmResponse> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        inner.analyze_mood(user_input)
    }
}
