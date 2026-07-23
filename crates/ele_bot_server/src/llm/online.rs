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
    ChatCompletionRequestAssistantMessageArgs, ChatCompletionRequestMessage,
    ChatCompletionRequestSystemMessageArgs, ChatCompletionRequestUserMessageArgs,
    CreateChatCompletionRequestArgs,
};
use async_openai::Client;
use std::collections::{HashMap, VecDeque};

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
    /// 对话历史记录：session_id -> 消息列表
    histories: HashMap<String, VecDeque<ChatCompletionRequestMessage>>,
    /// 当前会话 ID
    current_session: String,
    /// 历史消息容量
    history_capacity: usize,
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
    /// - `history_capacity`: 每个会话的历史消息最大数量
    pub fn new(
        api_base: &str,
        api_key: &str,
        model: &str,
        history_capacity: usize,
    ) -> Result<Self> {
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
            histories: HashMap::new(),
            current_session: "default".to_string(),
            history_capacity,
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

    /// 确保会话存在，不存在则创建
    fn ensure_session(&mut self) {
        if !self.histories.contains_key(&self.current_session) {
            self.histories
                .insert(self.current_session.clone(), VecDeque::new());
        }
    }

    /// 创建用户消息
    fn create_user_message(content: &str) -> ChatCompletionRequestMessage {
        ChatCompletionRequestUserMessageArgs::default()
            .content(format!("用户输入：{}", content))
            .build()
            .map(|m| m.into())
            .unwrap_or_else(|_| {
                ChatCompletionRequestUserMessageArgs::default()
                    .content(format!("用户输入：{}", content))
                    .build()
                    .unwrap()
                    .into()
            })
    }

    /// 创建助手消息
    fn create_assistant_message(content: &str) -> ChatCompletionRequestMessage {
        ChatCompletionRequestAssistantMessageArgs::default()
            .content(content.to_string())
            .build()
            .map(|m| m.into())
            .unwrap_or_else(|_| {
                ChatCompletionRequestAssistantMessageArgs::default()
                    .content(content.to_string())
                    .build()
                    .unwrap()
                    .into()
            })
    }

    /// 添加消息到历史记录
    fn add_message_to_history(&mut self, msg: ChatCompletionRequestMessage) {
        self.ensure_session();
        // Safety: session is guaranteed to exist after ensure_session()
        let history = self.histories.get_mut(&self.current_session).unwrap();
        history.push_back(msg);
        // 超过容量时移除最旧的消息
        while history.len() > self.history_capacity {
            history.pop_front();
        }
    }

    /// 构建完整的消息列表（含历史）
    fn build_messages_with_history(&self, user_prompt: &str) -> Vec<ChatCompletionRequestMessage> {
        let mut messages = vec![self.system_message.clone()];
        if let Some(history) = self.histories.get(&self.current_session) {
            messages.extend(history.iter().cloned());
        }
        messages.push(Self::create_user_message(user_prompt));
        messages
    }

    /// 异步执行情感分析（内部实现）
    ///
    /// # 错误处理
    /// 返回的错误可能来自：
    /// - 网络连接问题
    /// - API 请求超时
    /// - API 返回错误（如认证失败、限流等）
    async fn analyze_mood_async(&mut self, user_input: &str) -> Result<LlmResponse> {
        let messages = self.build_messages_with_history(user_input);

        // 构建请求
        let request = CreateChatCompletionRequestArgs::default()
            .model(&self.model)
            .messages(messages.clone())
            .temperature(0.0)
            .max_tokens(512u32)
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
            .unwrap_or_else(|| {
                log::warn!("response is none");
                String::from("[中性]")
            });

        log::info!("Online LLM response: {}", content);

        // 分离情感和动作
        let (mood_str, actions_str) = Self::split_response(&content);
        let mood = parse_mood(mood_str);
        let actions = parse_actions(actions_str);

        log::info!("Mood: {:?}, Actions count: {}", mood, actions.len());
        actions
            .iter()
            .for_each(|action| log::info!("Action: {action:?}"));

        // 保存 user message 到历史
        self.add_message_to_history(Self::create_user_message(user_input));

        // 保存 assistant message 到历史
        self.add_message_to_history(Self::create_assistant_message(&content));

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
}

impl LlmTrait for OnlineLlm {
    fn analyze_mood(&mut self, user_input: &str) -> Result<LlmResponse> {
        // 由于调用方可能在非 async 上下文（如 std::thread）中调用，
        // 需要创建新的 Tokio runtime 来执行异步代码
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(self.analyze_mood_async(user_input))
    }

    fn set_session_id(&mut self, session_id: &str) {
        self.current_session = session_id.to_string();
        self.ensure_session();
    }

    fn clear_session_history(&mut self, session_id: &str) {
        if let Some(history) = self.histories.get_mut(session_id) {
            history.clear();
        }
    }

    fn clear_all_histories(&mut self) {
        self.histories.clear();
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
