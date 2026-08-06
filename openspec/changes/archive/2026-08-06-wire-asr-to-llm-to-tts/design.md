## Context

### 现状 (你问 "ASR 到 LLM 真通了吗" 的真相)

```
   麦克风 → cpal → audio_tx → recognition_loop → result_tx.send(text)
                                                          ↓
                                                     mpsc::channel (capacity=0)
                                                          ↓
                                                  _rx: text_rx 字段占位, 无人 recv
                                                          ↓
                                                  VoiceManager drop 时 channel 关
                                                          ↓
                                                  ASR 文本永久消失

   TUI 客户端 send_llm_text (input/llm_test.rs:23)  ← 唯一发送方
        ↓ WebSocket ClientMessage::SendLlmText
   ws.rs:239-242  llm_text_tx.send(text)
        ↓
   state.rs:289 spawn_llm_thread text_rx ← 唯一接收方
        ↓
   llm.analyze_mood(text)  ← 只输出 Mood + Vec<Action>, 不生成对话文本
        ↓
   LCD 表情 + 舵机动作. 没有 TTS, 没有 LLM 文字回复.
```

TTS 链路更孤立: `ClientMessage::TtsSpeak` 是唯一入口 (`ws.rs:244`), 服务端自己**没有任何代码**调 `voice.speak()`.

LLM 能力差异:
- `OnlineLlm` (async_openai): 已经有完整 `histories: HashMap<session_id, VecDeque<msg>>` + `system_prompt()` + `set_session_id` / `clear_session_history` 实现, 调 chat completions API
- `QwenLlm` (candle 本地): `analyze_mood` 只返 Mood, prompt 是手写情感分类 prompt ("情感选项: 开心、难过..."), `set_session_id` / `clear_session_history` / histories 全是 trait default 空实现 — 等于无状态单轮

## Goals / Non-Goals

**Goals:**
- ASR 文本真正被消费, 走完 LLM → TTS 全链路
- LLM 输出对话文本 (不是只分类), 由 `LlmTrait::chat` 暴露
- `spawn_llm_thread` 拿到 LLM 回复后调 `voice.speak()`
- TUI 端能看到 LLM 实际回复 (扩展 `ServerEvent::LlmResponse` 协议字段)
- 不破坏现有 TUI 手动触发 (`SendLlmText` / `TtsSpeak` 仍工作)
- 在线 / 本地两种 LLM 都能跑 chat

**Non-Goals:**
- 不实现对话记忆 (QwenLlm 单轮足够验证链路通)
- 不实现工具调用 / function calling
- 不实现主动打断 TTS (用户说话打断正在播放的 TTS)
- 不实现流式 TTS 接入 LLM 增量生成
- 不动 ASR / VAD (commit d841409 / e17fd6a 修好的)

## Decisions

### D1: VoiceManager 暴露 ASR text 出去 (截 1 修复)

**当前**: `voice/mod.rs:155 _rx: text_rx` 字段占位, channel 内文本堆积至 drop.

**方案**: VoiceManager 加字段 `asr_text_tx: Option<mpsc::UnboundedSender<String>>`, `rebuild_voice` 时由 SharedState 注入. ASR 识别线程把识别结果 send 到 `result_tx` (asr.rs) 同时也 send 到 `asr_text_tx`. SharedState 在 `asr_text_rx` 收到文本后转发到现有 `llm_text_tx`.

**不**用单 channel 共享: 因为 `VoiceManager` 是热重建 (`rebuild_voice` 重建), 旧实例 drop 时 `_rx` channel 会关, 新实例会新建一个. `SharedState` 应该订阅最新实例的 channel, 用 `Option<Sender>` 注入保证始终指向活实例.

### D2: LlmTrait 加 `chat()` 接口 (截 2 部分)

```rust,ignore
pub trait LlmTrait: Send {
    fn analyze_mood(&mut self, user_input: &str) -> Result<LlmResponse>;
    /// 生成对话文本回复
    fn chat(&mut self, user_input: &str) -> Result<String>;

    fn set_session_id(&mut self, _session_id: &str) {}
    fn clear_session_history(&mut self, _session_id: &str) {}
    fn clear_all_histories(&mut self) {}
}
```

**保留 `analyze_mood`**: 现有舵机动作 + 表情走这条; 新 `chat` 返文本. `spawn_llm_thread` 改调 `chat` 拿文本, 但**保留**对 `analyze_mood` 的调用 (拿 mood/actions 走 LCD/舵机).

**理由**: 一阶段完成两件事会让 LLM prompt 变复杂 ("你既是情感分类器又是对话助手"), 拆成两阶段调用简单清晰. `analyze_mood` 输入短 (8 tokens max), 不会显著拖慢.

### D3: spawn_llm_thread 改调 chat + 串 TTS

```rust,ignore
while let Some(text) = text_rx.blocking_recv() {
    if text.is_empty() { continue; }
    state.llm_processing.store(true, ...);
    emit LlmProcessing { is_processing: true };

    // 阶段 1: 生成对话文本 (新增)
    let reply_text = {
        let mut llm = state.llm.lock().unwrap();
        llm.chat(&text).unwrap_or_else(|e| {
            log::warn!("chat failed: {e:?}");
            format!("[LLM 错误: {}]", e)
        })
    };

    // 阶段 2: 情感分类 + 动作 (保留)
    let response = {
        let mut llm = state.llm.lock().unwrap();
        llm.analyze_mood(&text).unwrap_or_else(|e| {
            log::warn!("analyze_mood failed: {e:?}");
            LlmResponse::default()
        })
    };

    emit LlmProcessing { is_processing: false };

    let proto = ProtoLlmResponse { mood, actions, reply_text };
    emit ServerEvent::LlmResponse { response: proto };

    // TTS 播报
    if !reply_text.is_empty() {
        if let Some(voice) = state.voice.lock().unwrap().clone() {
            tokio::task::spawn_blocking(move || {
                if let Err(e) = voice.speak(&reply_text, 1.0, None) {
                    log::warn!("TTS playback failed: {e:?}");
                }
            });
        }
    }

    if let Ok(mut lcd) = state.lcd.lock() {
        lcd.set_eyes_mood(response.mood);
    }
}
```

### D4: QwenLlm::chat 实现

复用现有 `QwenLlm::generate(prompt, max_tokens)`. Prompt 模板:

```rust,ignore
fn build_chat_prompt(user_input: &str) -> String {
    format!(
        "system\n你是一个桌面机器人, 用简短中文回复 (≤ 30 字).\nuser\n{user_input}\n\nassistant\n"
    )
}
```

max_tokens = 64 (短回复). 不实现会话历史 (QwenLlm 单轮).

### D5: OnlineLlm::chat 实现

`OnlineLlm` 已经有 `histories: HashMap<String, VecDeque<ChatCompletionRequestMessage>>` + `system_message`. 新增 `build_chat_messages_with_history(user_input)` 类似 `build_messages_with_history`, 但 system_message 是 "你是一个桌面机器人, 用简短中文回复 (≤ 30 字)." 而非情感分类 prompt. 走 chat completions API, 拿到 `content` 后 add_message_to_history (user + assistant), 返回 content.

### D6: 协议层加 reply_text 字段

`ServerEvent::LlmResponse.response: ProtoLlmResponse` 加 `reply_text: Option<String>`. 旧客户端忽略 None. 文档说明此字段为 LLM 实际回复文本, TUI 可在 LLM 测试页显示.

## Risks / Trade-offs

- [R1: ASR 文本被服务端自动送 LLM 后, TUI 手动 SendLlmText 重复送同一句] → Mitigation: 协议层加可选 source 字段 (`Asr` / `Tui`), `spawn_llm_thread` 据此打不同日志. **不在本次范围**, 简化为都走同条路.
- [R2: LLM chat + analyze_mood 串行调用, 慢] → Mitigation: 后续 PR 可并行 (`tokio::join!`). 本次先串行验证链路通.
- [R3: TTS 正在播报时新 ASR 文本进来, 互相打断] → 当前 `voice.speak` 是同步的, 调用者 `spawn_blocking` 阻塞到播完. 新文本堆积在 `llm_text_tx` channel, 排队播. 不做打断 — 后续 PR.
- [R4: QwenLlm 单轮无历史, 对话体验差] → Mitigation: 文档说明本次只验证链路通, 对话记忆是 `wire-llm-conversation-history` 后续 change.
- [R5: 协议层加字段需同步 client / server 两侧 Proto 类型] → Mitigation: 用 Option<String> + serde default, 旧 ws 消息兼容.

## Migration Plan

无. 服务端热重启, 协议字段是可选. TUI 旧版本忽略 `reply_text` 字段, 仍能正常显示 mood/actions.

## Open Questions

- [O1] LLM prompt 模板中文还是英文? — 桌面机器人场景选中文
- [O2] max_tokens 上限? — QwenLlm 用 64 (短回复), OnlineLlm 用 80 留余地
- [O3] TTS 调用方要不要把 speed 做成可配置? — 当前 1.0 写死, 后续 PR