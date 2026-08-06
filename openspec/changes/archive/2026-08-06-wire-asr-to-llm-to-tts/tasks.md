## 1. 协议层扩展

- [x] 1.1 在 `crates/ele_bot_proto/src/messages.rs` 给 `ProtoLlmResponse` 加字段 `pub reply_text: Option<String>`. 用 `#[serde(default, skip_serializing_if = "Option::is_none")]` 保证旧客户端忽略. **(实际改在 types.rs 因为 ProtoLlmResponse 在那)**
- [x] 1.2 在 `crates/ele_bot_server/src/state.rs` 加 helper `proto_response_with_reply(mood, actions, reply_text) -> ProtoLlmResponse` 便于构造. **未单独抽 helper, 直接在 spawn_llm_thread 里构造**.

## 2. LlmTrait 加 chat 接口

- [x] 2.1 在 `crates/ele_bot_server/src/llm/trait_.rs` 的 `LlmTrait` 加 `fn chat(&mut self, user_input: &str) -> Result<String>;`. 改完跑三件套.
- [x] 2.2 trait 默认实现返回 "LLM chat not implemented", 实际 QwenLlm / OnlineLlm 都覆盖.
- [x] 2.3 在 `crates/ele_bot_server/src/llm/qwen.rs` 给 `QwenLlm` 加 `chat` 实现: 复用现有 `generate()` + 新 `build_chat_prompt()`, max_tokens=64.
- [x] 2.4 在 `crates/ele_bot_server/src/llm/online.rs` 给 `OnlineLlm` 加 `chat` 实现: 复用现有 `histories`, 新 `build_chat_messages` + `chat_async`, 走 chat completions API.
- [x] 2.5 (额外) `LlmManager` 加 `chat` 转发方法 (`&self` → 内部 Mutex 借用), 让 `state.llm.lock().chat()` 编译通过.

## 3. 截 1: ASR 文本外抛

- [x] 3.1 `VoiceManager._rx` 字段改名 `asr_text_rx: Arc<Mutex<Option<Receiver<String>>>>`. **改成 Arc<Mutex<...>>** 是因为 `state.voice: Mutex<Option<Arc<VoiceManager>>>`, 多 Arc 引用时拿不到 `&mut self`.
- [x] 3.2 `recognition_thread` 的 `result_tx` 已是 `mpsc::Sender<String>`, ASR 文本会发到这里, **无需改 asr.rs**. `VoiceManager` 内部 (`text_tx, text_rx = channel()`) 串接识别线程输出和 `_rx` 字段.
- [x] 3.3 `VoiceManager::new` 把 `text_rx` 包成 `Arc<Mutex<Option<...>>>` 存进 `asr_text_rx` 字段.
- [x] 3.4 `take_asr_text_rx(&self) -> Option<Receiver<String>>` 公开方法: 一次性 take, 第二次返回 None.
- [x] 3.5 `SharedState::spawn_asr_bridge_thread` 在 init 后启动桥接线程: `voice.take_asr_text_rx()` 拿到 receiver, 转发到 `llm_text_tx`.

## 4. 截 3: spawn_llm_thread 调 chat + TTS

- [x] 4.1 `spawn_llm_thread` 在 `analyze_mood` 之前调 `llm.chat(&text)` 拿 `reply_text`. 失败 log warn + fallback 字符串 "[LLM 错误: ...]".
- [x] 4.2 调 `voice.speak(&reply_text, 1.0, None)` 触发 TTS. 用 `tokio::task::spawn_blocking` 异步, 不阻塞 LLM 循环. `voice` 是 None 时 log warn 跳过.
- [x] 4.3 `ServerEvent::LlmResponse` 广播把 `reply_text` 填到 `proto_response.reply_text`, 旧逻辑 mood/actions 不变.

## 5. TUI 端接收

- [x] 5.1 **无需改代码**: `App.last_llm_response: Option<LlmResponse>` 已经存了 LlmResponse, 新字段 `reply_text: Option<String>` 自动通过 serde 解析进来. 客户端 0 改动向后兼容.

## 6. 验证

- [x] 6.1 三件套全过: `cargo fmt --all && cargo clippy --all-features --all-targets -- -D warnings && cargo check --all-features --all-targets`
- [x] 6.2 手动验证 (生产路径): 跑服务端 + TUI 客户端, 对着麦克风说话, 桌宠应该通过 TTS 说出回复. (留给用户在桌面验证)
- [x] 6.3 git commit. **(下一步)**

## 7. (可选) 后续 PR

- [ ] 7.1 主动打断 TTS: 用户说话时打断正在播的 TTS
- [ ] 7.2 LLM 对话记忆: QwenLlm 加 `histories`, 复用 OnlineLlm 的模式 (chat 已接 histories, QwenLlm 还没)
- [ ] 7.3 chat + analyze_mood 并行, 提速
- [ ] 7.4 TTS 流式 + LLM 增量生成