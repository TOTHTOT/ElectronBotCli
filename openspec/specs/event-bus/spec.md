# event-bus Specification

## Purpose
TBD - created by archiving change event-bus-refactor. Update Purpose after archive.
## Requirements
### Requirement: EventBus 提供 publish + subscribe 接口

`crates/ele_bot_server/src/event_bus.rs` MUST 暴露 `EventBus::new(capacity)` 构造 + `publish(event)` 发送 + `subscribe()` 拿 `tokio::sync::broadcast::Receiver<BusEvent>`. `publish` 失败 (无订阅者 / 容量满) MUST 不 panic, 只 log warn. `subscriber_count()` MUST 返回活跃订阅者数.

#### Scenario: 单订阅者收到 publish 的事件
- **WHEN** 调 `bus.publish(BusEvent::AsrText("hello".into()))`
- **THEN** 同一 bus 的订阅者 `rx.recv()` 收到 `BusEvent::AsrText("hello")`

#### Scenario: 多订阅者各自独立
- **WHEN** 两个订阅者 A 和 B 同时 subscribe
- **THEN** `publish` 后 A 和 B 各自独立收到事件, 一方 Lagged 不影响另一方

#### Scenario: Lagged 处理
- **WHEN** 订阅者太慢, broadcast 容量满
- **THEN** 订阅者下次 `recv` 返回 `Err(Lagged(n))`, 跳到最新事件继续

### Requirement: BusEvent 枚举覆盖所有事件流场景

`BusEvent` MUST 至少包含这些 variant:
- `ServerEvent(ServerEvent)` — TUI 显示用
- `AsrText(String)` — ASR 识别结果, LLM 消费
- `LlmReply(String)` — LLM 对话回复, TTS 消费
- `LlmProcessing { is_processing: bool }` — LLM 处理中标志
- `Volume(i32)` — 实时音量, 替代 ws 50ms tick 轮询

新增 variant MUST 加在这里而不是散落各模块.

#### Scenario: 枚举变体覆盖需求场景
- **WHEN** 读 `crates/ele_bot_server/src/event_bus.rs::BusEvent`
- **THEN** 至少包含 `ServerEvent` / `AsrText` / `LlmReply` / `LlmProcessing` / `Volume` 五个 variant

### Requirement: 事件流走 EventBus, 数据流保留原 channel

`crates/ele_bot_server/src` MUST 把所有"事件流"改走 `EventBus.publish/subscribe`. 但以下"数据流"通道 MUST **保留原 channel**, 不走 bus:
- cpal 音频流 `sync_channel<Vec<f32>>(4)`
- 摄像头帧 `broadcast<FrameInfo>(100)`
- WebSocket 单连接双向 `mpsc::unbounded_channel<ServerEvent>` 和 `sync_channel<(Vec<u8>, JointConfig)>(1)`
- USB 关节指令 `sync_channel(1)`

#### Scenario: 事件流走 bus
- **WHEN** ASR 识别出文本, LLM 处理完得到 reply_text, ws 客户端在线
- **THEN** 文本从 ASR thread 经 `bus.publish(AsrText(...))` 到 LLM tokio task, 再 `bus.publish(LlmReply(...))` 到 TTS trigger thread, 三个环节都通过 bus, 不再有专用 mpsc

#### Scenario: 数据流不走 bus
- **WHEN** cpal 推音频帧, 摄像头推视频帧, ws 客户端 send/recv
- **THEN** 仍走原来各自的 sync_channel / broadcast / mpsc, EventBus 不参与

### Requirement: LLM 处理从 std thread 迁 tokio task

`SharedState::spawn_llm_thread` MUST 从 `std::thread::spawn` + `mpsc::blocking_recv` 改为 `tokio::spawn` + `bus.subscribe().recv().await`. 内部 LLM 调用 (`llm.chat` / `llm.analyze_mood`) 保持同步 Mutex 借用 (短临界区), 不在 await 点持锁.

#### Scenario: LLM tokio task 订阅 AsrText
- **WHEN** bus.publish(AsrText("你好".into()))
- **THEN** LLM tokio task 在 ≤100ms 内开始处理, 输出 reply_text 经 bus.publish(LlmReply(...)) 流转

#### Scenario: LLM Lagged 不 panic
- **WHEN** bus 容量满, LLM 处理慢
- **THEN** LLM task 收到 Err(Lagged(n)), log warn + 跳到下一条事件继续, 不退出

### Requirement: TTS 触发独立线程

`SharedState` MUST 有 `spawn_tts_trigger_thread`, 订阅 `BusEvent::LlmReply`, 调 `voice.speak(&text, 1.0, None)`. 调用 MUST 用 `tokio::task::spawn_blocking` 异步, 不阻塞 bus 消费循环. `voice` 不可用 (None) 时 log warn 跳过.

#### Scenario: LlmReply 触发 TTS
- **WHEN** bus.publish(LlmReply("你好呀".into()))
- **THEN** 1-3 秒内从输出设备听到 TTS 播报 "你好呀"

#### Scenario: voice 不可用跳过
- **WHEN** TTS trigger thread 收到 LlmReply 但 state.voice 是 None
- **THEN** log warn "voice manager not available for TTS", 继续处理下一条事件

### Requirement: WebSocket 客户端订阅 bus 过滤外发

`ws.rs::handle_connection` MUST 订阅 `state.bus_tx.subscribe()`, 按 BusEvent variant 过滤:
- `BusEvent::ServerEvent(se)` → 直接序列化发给 WS 客户端
- `BusEvent::Volume(v)` → 转 `ServerEvent::Volume { value: v }` 发给客户端
- `BusEvent::AsrText(_)` / `BusEvent::LlmReply(_)` / `BusEvent::LlmProcessing(_)` → 内部用, 不外发 (TUI 不关心)

50ms tick 轮询 `state.voice.volume()` 那段 MUST 删除 (改由 voice 主动 publish `BusEvent::Volume`).

#### Scenario: WS 收到 ServerEvent
- **WHEN** 任何模块 `bus.publish(BusEvent::ServerEvent(连接状态))`
- **THEN** 所有 WS 客户端收到该 ServerEvent 的 JSON 序列化

#### Scenario: WS 不收到 AsrText
- **WHEN** `bus.publish(BusEvent::AsrText("hello".into()))`
- **THEN** WS 客户端不收到该事件 (内部 LLM 消费, 不外发)

#### Scenario: 音量推送改走 bus
- **WHEN** `recognition_loop` / 音频 chunk 处理时更新音量
- **THEN** 通过 `bus.publish(BusEvent::Volume(v))` 推送, 不再 50ms tick 轮询

### Requirement: VoiceManager 删除 ASR 文本外抛 channel

`VoiceManager` MUST 删除以下字段和方法:
- `_rx: mpsc::Receiver<String>` 占位字段
- `asr_text_rx: Arc<Mutex<Option<mpsc::Receiver<String>>>>` 字段
- `take_asr_text_rx()` 方法

`recognition_thread` MUST 接收 `EventBus` 引用作为新参数, 识别出非空文本时 `bus.publish(BusEvent::AsrText(text))` (替代 `result_tx.send(text)`). `text_tx/text_rx` channel 删除.

#### Scenario: ASR 文本经 bus 流向 LLM
- **WHEN** ASR 识别出 "你好"
- **THEN** recognition_thread 直接 `bus.publish(AsrText("你好"))`, 没有中间 channel 环节, LLM tokio task 收到

#### Scenario: VoiceManager 没有 ASR 文本 channel
- **WHEN** 读 `crates/ele_bot_server/src/media/voice/mod.rs`
- **THEN** 不存在 `asr_text_rx` / `_rx` / `take_asr_text_rx` 字段或方法

