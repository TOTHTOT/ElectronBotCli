## Why

ASR 识别出的文本 (`crates/ele_bot_server/src/media/voice/asr.rs::recognition_loop`) 当前通过 `mpsc::channel` 推到 `VoiceManager._rx` 字段就**没人 recv**, 文本堆积在 channel 里直到 VoiceManager drop. 同时 `SharedState::spawn_llm_thread` 跑的 `llm_text_tx` 链路**只被 TUI 客户端手动 `send_llm_text` 触发** (`crates/ele_bot_client/src/input/llm_test.rs`), ASR 输出根本没接进 LLM. 而且 LLM 现有 `LlmTrait::analyze_mood` 只输出 `Mood + Vec<Action>`, 不生成对话文本. TTS 也只能被 TUI 客户端通过 `ClientMessage::TtsSpeak` 手动触发. 结果是: 说话 → ASR 识别 → 文本消失, LLM 永远收不到真实语音, 桌宠没有语音回复.

## What Changes

- `VoiceManager` 把 ASR 识别文本通过新字段 `asr_text_tx` 主动 push 到 `SharedState`, 不再吞在内部 channel
- `SharedState` 新增 `asr_text_rx` 接收, 内部桥接到现有的 `llm_text_tx` 链路, 让 ASR 文本走和 TUI 手动发一样的路径
- `LlmTrait` 加 `fn chat(&mut self, user_input: &str) -> Result<String>`, 在现有 `analyze_mood` 旁新增生成对话文本的能力
- `QwenLlm` 实现 `chat` (复用现有 `generate` + 新 `build_chat_prompt`), 给本地模型加简单 system prompt + 多 session 历史的最小骨架
- `OnlineLlm` 实现 `chat` (已有 histories + chat completions 能力, 加一个 prompt 走 `chat` 而不是 `analyze_mood`)
- `spawn_llm_thread` 改调 `chat()` 而不是 `analyze_mood()`; 拿到回复文本后调 `voice.speak()` 触发 TTS
- TUI 端 `ServerEvent::LlmResponse` 协议加可选字段 `reply_text`, 让客户端能看到 LLM 实际回复

## Capabilities

### New Capabilities

- `asr-llm-tts-pipeline`: 端到端 "ASR 识别 → LLM 生成对话 → TTS 播报" 闭环跑通, 用户对着麦克风说话能听到桌宠语音回复. 服务端自动处理, 不依赖 TUI 客户端手动触发中间环节.

### Modified Capabilities

无.

## Impact

- 改 crate: `crates/ele_bot_server` (voice/mod.rs, state.rs, llm/{trait_,response,online,qwen}.rs, ws.rs) 和 `crates/ele_bot_proto` (messages.rs 加 ServerEvent 字段) 和 `crates/ele_bot_client` (app/mod.rs 接收 reply_text)
- 不引入新依赖 (复用现有 sherpa_onnx / candle / async_openai)
- 协议层加一个 optional 字段, 向后兼容旧客户端

## Non-goals

- 不做对话记忆/多 session 上下文 (QwenLlm 端先单轮跑通, 后续 PR 加)
- 不做工具调用 (function calling) / 日程 / 待办 / 备忘录 (README 桌宠功能里那些是后续 PR)
- 不改 TTS 流式接口 (`speak_streaming` 已有, 本次仍走 `speak`)
- 不动 VAD / ASR 识别逻辑 (commit d841409 / e17fd6a 已修)
- 不动协议层 client → server 方向 (TUI 仍可手动 SendLlmText / TtsSpeak)