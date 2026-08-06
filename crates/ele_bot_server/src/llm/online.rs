//! 在线 LLM 模块
//!
//! 使用 async-openai 调用在线 LLM API（如豆包/火山引擎）
//!
//! # 设计说明
//! - `OnlineLlm` 实现了 `Send + Sync`，可在多线程间共享
//! - 使用 `tokio::task::block_in_place` 在同步上下文中执行异步代码
//! - 请求默认超时 30 秒

use crate::emotion::{parse_actions, parse_mood};
use crate::llm::response::LlmResponse;
use crate::llm::trait_::LlmTrait;
use anyhow::{Context, Result};
use async_openai::config::OpenAIConfig;
use async_openai::types::chat::{
    ChatCompletionRequestMessage, ChatCompletionRequestSystemMessageArgs,
    ChatCompletionRequestUserMessageArgs, CreateChatCompletionRequestArgs,
};
use async_openai::Client;
use async_trait::async_trait;

/// 在线 LLM 实现
///
/// 使用 async-openai 库的异步客户端，支持任意兼容 `OpenAI` API 格式的后端。
/// 当前只承担 `analyze_mood` (情感/动作分析) 与无状态的兜底 `chat`;
/// 对话历史由 zeroclaw 托管 (specs/001-zeroclaw-llm-integration FR-002),
/// 本结构体 **不保存任何对话历史**.
///
/// # 线程安全
/// 该结构体实现了 `Send + Sync`，可以在多线程间共享同一个实例。
pub struct OnlineLlm {
    /// 异步 `OpenAI` 客户端
    client: Client<OpenAIConfig>,
    /// 模型名称
    model: String,
    /// 预构建的系统消息
    system_message: ChatCompletionRequestMessage,
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

        // 预构建系统消息
        let system_message = ChatCompletionRequestSystemMessageArgs::default()
            .content(Self::system_prompt())
            .build()?
            .into();

        Ok(Self {
            client,
            model: model.to_string(),
            system_message,
        })
    }

    /// 构建情感分析的 system prompt
    fn system_prompt() -> &'static str {
        // 明确要求输出格式与 parse_mood 的解析逻辑一致
        // parse_mood 使用 starts_with("[开心]") 等方式匹配
        r#"你是一个情感分析助手，同时负责生成机器人动作。

## 情感输出（必须）
根据用户输入判断情感，只输出情感标签：
- 开心：[开心]
- 难过：[难过]
- 生气：[生气]
- 惊讶：[惊讶]
- 困惑：[困惑]
- 中性或其他：[中性]

## 动作输出（可选）
根据用户输入生成动作，格式为JSON数组。
**重要：动作必须包含完整的流程（抬起→执行→放下），每个动作完成后要恢复到0度。**

### 舵机对应关系
- 0: 头部 (范围: -15° ~ 15°)
- 1: 左肩 (范围: -30° ~ 30°)
- 2: 左臂 (范围: -180° ~ 180°)
- 3: 右肩 (范围: -30° ~ 30°)
- 4: 右臂 (范围: -180° ~ 180°)
- 5: 身体 (范围: -90° ~ 90°)

### 完整动作示例
1. 右手挥手：抬起(90°) → 摆动1(60°) → 摆动2(90°) → 摆动3(60°) → 放下(0°)
   [{"servo": 4, "angle": 90, "duration": 200}, {"servo": 4, "angle": 60, "duration": 150}, {"servo": 4, "angle": 90, "duration": 150}, {"servo": 4, "angle": 60, "duration": 150}, {"servo": 4, "angle": 0, "duration": 200}]
2. 点头：低头(10°) → 抬起(-10°) → 恢复(0°)
   [{"servo": 0, "angle": 10, "duration": 150}, {"servo": 0, "angle": -10, "duration": 150}, {"servo": 0, "angle": 0, "duration": 150}]
3. 摇头：左转(15°) → 右转(-15°) → 恢复(0°)
   [{"servo": 0, "angle": 15, "duration": 150}, {"servo": 0, "angle": -15, "duration": 150}, {"servo": 0, "angle": 0, "duration": 150}]
4. 身体左转：左转(45°) → 恢复(0°)
   [{"servo": 5, "angle": 45, "duration": 300}, {"servo": 5, "angle": 0, "duration": 300}]

## 输出格式
先输出情感标签，换行后输出动作JSON（无可用动作时输出 []）：
[情感]
[{"servo": X, "angle": Y, "duration": Z}, ...]
"#
    }

    /// 创建用户消息
    fn create_user_message(content: &str) -> ChatCompletionRequestMessage {
        ChatCompletionRequestUserMessageArgs::default()
            .content(format!("用户输入：{content}"))
            .build()
            .map(std::convert::Into::into)
            .unwrap_or_else(|_| {
                ChatCompletionRequestUserMessageArgs::default()
                    .content(format!("用户输入：{content}"))
                    .build()
                    .unwrap()
                    .into()
            })
    }

    /// 异步执行情感分析（内部实现）
    ///
    /// 单发请求 (system + user), 不携带历史: 历史由 zeroclaw 托管后,
    /// 情感分析只需要当前输入 (spec: FR-002).
    ///
    /// # 错误处理
    /// 返回的错误可能来自：
    /// - 网络连接问题
    /// - API 请求超时
    /// - API 返回错误（如认证失败、限流等）
    async fn analyze_mood_async(&mut self, user_input: &str) -> Result<LlmResponse> {
        let messages = vec![
            self.system_message.clone(),
            Self::create_user_message(user_input),
        ];

        // 构建请求
        let request = CreateChatCompletionRequestArgs::default()
            .model(&self.model)
            .messages(messages.clone())
            .temperature(0.0)
            .max_tokens(512u32)
            .build()?;

        log::debug!(
            "Online LLM request: {:#?}",
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
            .unwrap_or_else(|| {
                log::warn!("response is none");
                String::from("[中性]")
            });

        log::info!("Online LLM response: {content}");

        // 分离情感和动作
        let (mood_str, actions_str) = Self::split_response(&content);
        let mood = parse_mood(mood_str);
        let actions = parse_actions(actions_str);
        log::info!("Mood: {:?}, Actions count: {}", mood, actions.len());
        for action in &actions {
            log::info!("Action: {action:?}");
        }

        Ok(LlmResponse { mood, actions })
    }

    /// 分离 LLM 输出中的情感标签和动作 JSON
    fn split_response(content: &str) -> (&str, &str) {
        // 查找第一个换行或动作 JSON 起始位置
        let lines: Vec<&str> = content.lines().collect();
        if lines.len() >= 2 {
            (lines[0].trim(), lines[1].trim())
        } else if content.contains('[') && content.contains("servo") {
            ("[中性]", content.trim())
        } else {
            (content.trim(), "[]")
        }
    }

    /// 对话 system prompt. 用 ≤ 30 字简短中文回复, 不要解释, 不要 markdown.
    fn chat_system_prompt() -> &'static str {
        "你是一个桌面机器人, 用简短中文回复用户 (≤ 30 字). 不要解释, 不要 markdown."
    }

    /// chat 用: 构造 [system + user] 消息序列 (无状态单发, 历史由 zeroclaw 托管).
    fn build_chat_messages(&self, user_input: &str) -> Vec<ChatCompletionRequestMessage> {
        vec![
            ChatCompletionRequestSystemMessageArgs::default()
                .content(Self::chat_system_prompt())
                .build()
                .expect("system message build")
                .into(),
            Self::create_user_message(user_input),
        ]
    }

    /// chat 异步实现: 无状态单发, 仅作兜底/调试用 (生产 chat 走 zeroclaw).
    async fn chat_async(&mut self, user_input: &str) -> Result<String> {
        let messages = self.build_chat_messages(user_input);
        let request = CreateChatCompletionRequestArgs::default()
            .model(&self.model)
            .messages(messages)
            .temperature(0.7)
            .max_tokens(80u32)
            .build()?;
        let response = self
            .client
            .chat()
            .create(request)
            .await
            .context("LLM chat API 调用失败")?;
        let content = response
            .choices
            .first()
            .and_then(|c| c.message.content.clone())
            .unwrap_or_default();
        Ok(content.trim().to_string())
    }
}

#[async_trait]
impl LlmTrait for OnlineLlm {
    async fn analyze_mood(&mut self, user_input: &str) -> Result<LlmResponse> {
        self.analyze_mood_async(user_input).await
    }

    async fn chat(&mut self, user_input: &str) -> Result<String> {
        self.chat_async(user_input).await
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
