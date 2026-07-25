## 1. EventBus 骨架

- [ ] 1.1 新建 `crates/ele_bot_server/src/event_bus.rs`, 定义 `BusEvent` 枚举 + `EventBus` 封装 (`new` / `publish` / `subscribe` / `subscriber_count`). 改完跑三件套.
- [ ] 1.2 在 `crates/ele_bot_server/src/lib.rs` 加 `pub mod event_bus;` 让它能从 main.rs / state.rs 引用. 改完跑三件套.

## 2. State 集成 EventBus

- [ ] 2.1 `SharedState` 加字段 `bus_tx: EventBus`, `new()` 里初始化 `EventBus::new(1024)`. 替换原 `event_tx: broadcast::Sender<ServerEvent>`.
- [ ] 2.2 删除 `SharedState::llm_text_tx: Mutex<Option<mpsc::UnboundedSender<String>>>` 字段 + 相关构造 (state.rs:128).
- [ ] 2.3 删除 `SharedState::spawn_asr_bridge_thread` 整个函数 + `state.rs::new` 里 `state.spawn_asr_bridge_thread()` 调用. (改完跑三件套)

## 3. VoiceManager 简化

- [ ] 3.1 删除 `VoiceManager._rx` 字段 + `text_rx` 在 `new()` 里的构造.
- [ ] 3.2 删除 `VoiceManager.asr_text_rx` 字段 + `take_asr_text_rx()` 方法.
- [ ] 3.3 `recognition_thread` 加参数 `bus: &EventBus`, 删 `result_tx: mpsc::Sender<String>` 参数. 识别出非空文本时 `bus.publish(BusEvent::AsrText(text))` (替代 send).
- [ ] 3.4 `VoiceManager::new` 不再接收 audio_rx/text_tx 创建逻辑, 改成接收 `bus: EventBus`. `thread::spawn` 调用 `recognition_thread` 时把 `bus` 传进去.

## 4. spawn_llm_thread 迁 tokio

- [ ] 4.1 `SharedState::spawn_llm_thread` 从 `std::thread::spawn + mpsc::blocking_recv` 改为 `tokio::spawn + bus.subscribe().recv().await`. 循环体内只处理 `BusEvent::AsrText(text)`, 其它 variant continue.
- [ ] 4.2 LLM 处理流程保留 (chat → analyze_mood → proto_response → ServerEvent::LlmResponse 广播 + voice.speak). 但把 `voice.speak` 那段抽到独立 `spawn_tts_trigger_thread`.
- [ ] 4.3 LLM 回复通过 `bus.publish(BusEvent::LlmReply(text))` 发布, 替代内部直调 voice.speak.

## 5. spawn_tts_trigger_thread

- [ ] 5.1 新增 `SharedState::spawn_tts_trigger_thread`, 订阅 `BusEvent::LlmReply`, 调 `voice.speak(&text, 1.0, None)`. `tokio::task::spawn_blocking` 异步. `voice` 为 None 时 log warn 跳过.
- [ ] 5.2 在 `SharedState::new` 末尾调 `state.spawn_tts_trigger_thread()`.

## 6. ws.rs 订阅过滤

- [ ] 6.1 ws.rs::handle_connection 订阅 `state.bus_tx.subscribe()`, 按 BusEvent variant 过滤 (`ServerEvent` / `Volume` 转 `ServerEvent::Volume`, `AsrText` / `LlmReply` / `LlmProcessing` continue).
- [ ] 6.2 删除 ws.rs:140 50ms tick 推音量那段 (`let volume = state.voice.lock()...; state.event_tx.send(ServerEvent::Volume { value: volume });`). 改由 voice / process_audio_chunk publish `BusEvent::Volume`.
- [ ] 6.3 ws.rs 保留 50ms tick 推 LCD 帧 (`state.generate_lcd_frame` + `push_frame_to_robot`), 不全删.

## 7. process_audio_chunk publish 音量

- [ ] 7.1 `process_audio_chunk` 接收 `EventBus` 引用, 音量更新后 `bus.publish(BusEvent::Volume(new_value))` (替代 None 时不推).

## 8. 测试

- [ ] 8.1 `event_bus.rs::tests`: 测试 `publish + subscribe` 单线程.
- [ ] 8.2 测试 `subscribe` 后 publish, receiver 收到. publish 后再 subscribe, 新 receiver 收不到 (旧 broadcast 行为).
- [ ] 8.3 测试多订阅者隔离 (sub A / sub B 各自 recv 互不干扰).
- [ ] 8.4 测试 Lagged 处理 (容量 2, 发 5 条, 旧订阅者 recv 看到 Lagged + 最新事件).
- [ ] 8.5 (可选) 集成测试: 模拟 ASR → bus → LLM stub → bus → TTS stub, 验证链路.
- [ ] 8.6 跑 `test_recognition_with_audio_file` / `test_recognition_no_lost_chars` 不应 regress.

## 9. 验证

- [ ] 9.1 三件套: `cargo fmt --all && cargo clippy --all-features --all-targets -- -D warnings && cargo check --all-features --all-targets` 必须全过.
- [ ] 9.2 `openspec validate event-bus-refactor --strict` 通过.
- [ ] 9.3 手动验证 (生产路径): 跑服务端 + TUI 客户端, 对着麦克风说话, TUI 看到 reply_text, 听到 TTS 播报. 行为与 commit e7833f1 等价.

## 10. 提交

- [ ] 10.1 git commit, 中文短句, 首行格式 `重构/引入 EventBus 统一事件流`.