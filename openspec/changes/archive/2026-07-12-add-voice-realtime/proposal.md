## Why

设置页切完麦克风后, 设备状态页 (DeviceStatus) 的"输入音量"条始终是 0, 不随用户说话变化; 同时手动切换麦克风看似生效, 但旧 ASR 识别线程仍在持有旧麦克风的数据, 偶发"换设备但识别仍在用旧麦"的问题. 两者同源于 `VoiceManager` 缺少对外可见的活跃状态.

## What Changes

- **协议**: 新增 `ServerEvent::Volume { value: i32 }` (0–100, 来自现有 `VoiceManager.volume` 的归一化峰值)
- **服务端**: `ws.rs` 复用既有的 `frame_interval` 50ms tick, 在同一个 `tokio::select!` 分支里读 `state.voice.volume()` 并广播 `Volume`. **不开新后台 task**, 不增加 thread 数量
- **VoiceManager 生命周期**: 新增 `running: Arc<AtomicBool>`, `recognition_loop` 改用 `recv_timeout(50ms)` 替代阻塞 `recv`, 每轮检查 `running` 标志. `rebuild_voice` 在替换前先把旧实例的 `running` 置 false, 旧 ASR 线程 50ms 内主动退出, 旧 cpal Stream 随 `VoiceManager` Drop 释放
- **客户端**: `apply_event` 处理 `ServerEvent::Volume`, 写入 `app.server.volume`; 既有 `device_status.rs` 渲染逻辑零改动

## 非目标 (Non-goals)

- 不改变音量采样算法 (峰值 + 指数衰减) 也不改变 VAD 阈值
- 不修改 ASR 模型加载路径
- 不给用户暴露音量调节控件 — 只做"显示"
- 不持久化 volume (它是运行时数据)

## Capabilities

### New Capabilities

- `add-voice-realtime`: 设备状态页输入音量实时刷新, 麦克风热切换立刻生效 — 协议 + 周期广播 + VoiceManager 取消信号

### Modified Capabilities

- `audio-device-picker`: 现有 capability 增加"重建时旧 ASR 线程主动退出"的隐含保证 (cancellation flag 语义). 行为层 contract 补充, 不是新 spec, 只在本 change 的 spec 里加一个 Scenario 引用.

## Impact

- `crates/ele_bot_proto/src/messages.rs`: 新增 `ServerEvent::Volume` 变体, 补 roundtrip 测试
- `crates/ele_bot_server/src/ws.rs`: `frame_interval` tick 分支加 volume 广播; 不开新 task
- `crates/ele_bot_server/src/media/voice/mod.rs`: `VoiceManager` 新增 `running: Arc<AtomicBool>`, 暴露 `pub fn running(&self) -> Arc<AtomicBool>` 与 `pub fn is_running(&self) -> bool`
- `crates/ele_bot_server/src/media/voice/asr.rs`: `recognition_thread` / `recognition_loop` 接受 `running` 参数, 改 `recv_timeout` + flag 检查
- `crates/ele_bot_server/src/state.rs`: `rebuild_voice` 重建前先 `if let Some(old) = guard.take() { old.running().store(false, Ordering::Relaxed); }`, 给旧实例一个最长 50ms 的退出窗口再覆盖
- `crates/ele_bot_client/src/app/mod.rs`: `apply_event` 加 `ServerEvent::Volume` 分支, 写 `server.volume`
- `crates/ele_bot_client/src/ui/pages/device_status.rs`: 渲染逻辑零改动 (已经读 `server.volume`)
