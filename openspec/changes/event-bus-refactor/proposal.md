## Why

项目里同时存在 4 类并发原语 (tokio::sync::broadcast / tokio::sync::mpsc / std::sync::mpsc / std::sync::mpsc::sync_channel) + 双重 Mutex 包装 Sender/Receiver 的奇怪模式, 散落在 5+ 模块里. 新加订阅者要改 4-5 处 wiring, debug 时不知道消息从哪来到哪去. 同时 `_rx` 占位字段 (`crates/ele_bot_server/src/media/voice/mod.rs:82`) 这种"漏的 channel"已经出过 bug (commit e7833f1).

事件流 (文本 / 关节指令 / 表情 / 音量 / 状态变化) 全部用 `tokio::sync::broadcast<BusEvent>` + 一个 `EventBus` 封装集中管理. 数据流 (cpal 音频 / 摄像头帧 / 单 WS 连接) 保留原 channel.

## What Changes

- 新模块 `crates/ele_bot_server/src/event_bus.rs`: 定义 `BusEvent` 枚举 + `EventBus` 封装 (内部 `broadcast::Sender<BusEvent>`)
- `SharedState.event_tx: broadcast::Sender<ServerEvent>` 改为 `bus_tx: EventBus`
- 删除 `SharedState.llm_text_tx: Mutex<Option<mpsc::UnboundedSender<String>>>` + `spawn_asr_bridge_thread`, 改成 LLM thread 订阅 `EventBus::AsrText` 事件
- 删除 `VoiceManager.asr_text_rx` + `take_asr_text_rx` 方法, ASR 识别结果直接 `bus.publish(AsrText(text))`
- 删除 `VoiceManager._rx` 占位字段 (上一 change 已注释, 这次彻底删)
- `SharedState::spawn_llm_thread` 改: 订阅 `BusEvent::AsrText` 触发处理, 发布 `BusEvent::LlmReply` (供 TTS 消费), LLM thread 改成 tokio task
- 新增 `spawn_tts_trigger_thread` 订阅 `BusEvent::LlmReply`, 调 `voice.speak()`
- ws.rs 订阅 bus, 按 variant 过滤, 只把该外发的转发给 socket
- `BusEvent::Volume` / `BusEvent::JointState` 等事件让 ws 直接订阅, 不用 50ms tick 轮询
- 加单元测试: `EventBus` publish + subscribe + 多订阅者隔离

## Capabilities

### New Capabilities

- `event-bus`: 服务端事件统一通过 `EventBus` 广播, 新订阅者只需 `bus.subscribe()` + `match` 过滤自己关心的 variant. 数据流 (audio / video frame / 单 WS 双向) 仍走专用 channel.

### Modified Capabilities

无.

## Impact

- 改 crate: `crates/ele_bot_server` (新增 `event_bus.rs`, 改 `state.rs` / `ws.rs` / `media/voice/{mod,asr}.rs` / 可能 `web/preview.rs`)
- 不动 `crates/ele_bot_client`, `crates/ele_bot_proto` (ServerEvent 协议不变, 只是内部走 bus)
- 不引入新依赖 (复用 `tokio::sync::broadcast`)
- 协议层零变化 (旧客户端不受影响)
- 运行时行为变化: 现有功能等价, 但新增订阅者成本从 O(改多文件) 降到 O(1 行 `subscribe()`)

## Non-goals

- 不改 cpal 音频流 channel (大流量数据, broadcast 语义不匹配)
- 不改 `frame_tx` 摄像头帧 broadcast (频率高, 独立 channel 合理)
- 不改 WebSocket 单连接双向 channel (`out_tx/out_rx`, `tx/rx`)
- 不改 USB 关节指令 `bot_tx` (强同步语义, sync_channel(1) 合适)
- 不重构 LLM 内部 (`LlmManager` / `LlmTrait`), 仅替换其触发方式
- 不引入 actor 框架, 不引入 crossbeam-channel
- 不改协议层 `ServerEvent` (bus 内部枚举用 `BusEvent`, 边界序列化仍走 `ServerEvent`)