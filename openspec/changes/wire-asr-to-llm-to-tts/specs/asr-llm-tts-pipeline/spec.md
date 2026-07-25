## ADDED Requirements

### Requirement: ASR 识别文本被消费并送 LLM

`SharedState` MUST 接收 `VoiceManager` 推送的 ASR 识别文本, 并把它转发到现有的 `llm_text_tx` 链路 (`spawn_llm_thread` 的 `text_rx` 接收方). 转发过程中 MUST NOT 丢失识别结果, 且 MUST NOT 阻塞 ASR 识别线程.

**理由**: 现状 ASR 文本在 `VoiceManager._rx` 字段里堆积到 drop 都没人 recv, 用户说话服务端没反应.

#### Scenario: ASR 识别结果进入 LLM 处理队列
- **WHEN** ASR 识别出一条文本 (例如 "你好")
- **THEN** 该文本被 `SharedState` 接收并转发到 `llm_text_tx`, `spawn_llm_thread` 在 ≤ 100ms 内开始处理

#### Scenario: ASR 文本为空时不触发 LLM
- **WHEN** ASR 识别出空字符串
- **THEN** `SharedState` 不转发到 `llm_text_tx`, LLM 线程不空转

### Requirement: LlmTrait 暴露 chat 接口生成对话文本

`LlmTrait` MUST 提供 `fn chat(&mut self, user_input: &str) -> Result<String>` 方法. `QwenLlm` 和 `OnlineLlm` 都 MUST 实现该方法, 返回字符串为 LLM 给用户的回复. 现有 `analyze_mood` MUST 保留, `chat` 是新增能力不是替代.

**理由**: 现状只有 `analyze_mood` (情感分类), 无法生成对话文本. 桌宠必须有"听到 → 思考 → 说"完整闭环.

#### Scenario: QwenLlm chat 返回中文短回复
- **WHEN** `QwenLlm::chat("你好")` 被调用
- **THEN** 返回非空字符串, 长度 ≤ 64 字 (max_tokens 上限), 中文优先

#### Scenario: OnlineLlm chat 走 chat completions API
- **WHEN** `OnlineLlm::chat("今天天气怎么样")` 被调用
- **THEN** 走 async_openai 的 chat completion, 把 user + assistant 都写入 `histories[current_session]`, 返回 assistant content

### Requirement: LLM 回复触发 TTS 播报

`spawn_llm_thread` 在拿到 LLM `chat()` 回复后 MUST 调 `voice.speak(&reply_text, 1.0, None)` 触发 TTS 播报, 调用 MUST 异步 (`spawn_blocking`) 不阻塞 LLM 处理循环. 若 `VoiceManager` 不可用 (热重建中) MUST 静默跳过并 log warn, 不 panic.

**理由**: TTS 是桌宠"发声"环节, 链路最后一公里不通则前面工作白做.

#### Scenario: LLM 回复后听到语音
- **WHEN** ASR 识别 "你好" → LLM chat 返回 "你好呀~"
- **THEN** 1-3 秒内从输出设备听到 TTS 播报 "你好呀~"

#### Scenario: VoiceManager 不可用时不 panic
- **WHEN** TTS 调用时 `state.voice` 是 None (热重建中)
- **THEN** log warn "voice manager not available for TTS", spawn_llm_thread 继续处理下一条

### Requirement: 协议层透传 LLM 回复文本

`ServerEvent::LlmResponse.response` (ProtoLlmResponse) MUST 新增 `reply_text: Option<String>` 字段. 服务端 MUST 在 LLM chat 完成后把 `reply_text` 填上并广播. 旧客户端忽略 None 字段不受影响 (向后兼容).

**理由**: TUI 端需要看到 LLM 实际回复, 调试和用户透明度都要.

#### Scenario: TUI 端看到 reply_text
- **WHEN** 服务端 LLM chat 返回 "今天天气不错"
- **THEN** TUI 端 `ServerEvent::LlmResponse` 收到 `reply_text: Some("今天天气不错")`

#### Scenario: 旧客户端兼容
- **WHEN** 旧版 TUI 客户端 (未升级) 收到带 reply_text 的消息
- **THEN** 旧客户端正常解析 mood / actions 字段, reply_text 被忽略不报错