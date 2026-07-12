# Capability: voice-realtime

设备状态页输入音量实时刷新, 音频设备热切换让旧 ASR 线程立刻退出.

## ADDED Requirements

### Requirement: 协议支持音量广播
系统 SHALL 提供一个 `ServerEvent::Volume { value: i32 }` 变体, 承载当前麦克风输入的归一化峰值音量 (0..=100). 客户端不需要单独请求; 字段值在 voice 不可用时为 0.

#### Scenario: 服务端广播音量
- **WHEN** 服务端持有 `Arc<VoiceManager>` 且 cpal 输入流在产生数据
- **THEN** 服务端 SHALL 每 50ms (20Hz) 通过 `event_tx` 广播一次 `ServerEvent::Volume`, `value` 等于 `voice.volume().load(Relaxed)`
- **AND** voice 不可用 (`state.voice` 为 `None`) 时, 广播的 `value` SHALL 为 0

#### Scenario: 客户端接收并缓存
- **WHEN** 客户端 `apply_event` 收到 `ServerEvent::Volume { value }`
- **THEN** 客户端 SHALL 将 `server.volume` 字段更新为 `value`
- **AND** 现有 `device_status.rs` 渲染逻辑 SHALL 无需改动即可看到音量条随说话变化

### Requirement: 客户端 UI 实时反映音量
设置页以外任意时刻 (例如设备状态页), 输入音量条 SHALL 反映最近一次收到的 `ServerEvent::Volume.value`. 收到新值前显示 0 (或上一帧值, 由渲染层决定).

#### Scenario: 用户在设备状态页说话
- **WHEN** 用户切到设备状态页, 设备状态页订阅 `server.volume`
- **THEN** 音量条 SHALL 随用户说话的音量峰值上下波动, 闲置时缓慢衰减到 0
- **AND** SHALL NOT 出现"始终显示 0"或"卡在旧值"的现象

### Requirement: VoiceManager 支持取消信号
`VoiceManager` SHALL 持有一个 `running: Arc<AtomicBool>`, 初始为 true. 客户端代码可调用 `running.store(false, Relaxed)` 来请求该实例的 ASR 线程退出.

ASR 识别线程 SHALL 在每次 `audio_rx` 等待时检查 `running`: 一旦为 false, 线程 SHALL 在最多 50ms 内主动退出 (返回 `Ok(())` 或 `Err`), 不再调用 sherpa-onnx 解码.

#### Scenario: 取消信号触发线程退出
- **WHEN** `running.store(false)` 被调用, ASR 线程正在 `audio_rx.recv_timeout(50ms)` 中等待
- **THEN** 线程 SHALL 在当前 50ms 等待超时后检测到 flag 并返回
- **AND** 旧 `VoiceManager` 的 cpal `Stream` 在 `Arc::drop` 链路上随实例一起释放

### Requirement: 重建 VoiceManager 时旧线程确认退出
`SharedState::rebuild_voice` SHALL 在替换 `self.voice` 之前, 把旧实例的 `running` 标志置 false; 新实例 SHALL 在旧实例退出窗口 (≤ 50ms) 之后再覆盖. 这样能保证:

1. 旧 cpal Stream 不会继续向 mpsc 写数据
2. 旧 ASR 线程不会继续占用 sherpa-onnx 解码资源
3. 系统中同时最多只有一个 ASR 实例在跑

#### Scenario: 切换麦克风时旧 ASR 线程退出
- **WHEN** 客户端发送 `SetConfig` 且 `config.speech_name` 与旧值不同
- **THEN** 服务端 `set_config` SHALL 调用 `rebuild_voice`
- **AND** `rebuild_voice` SHALL 先把旧 `VoiceManager` 的 `running` 置 false, 然后 sleep 50ms, 再 `lock+replace`
- **AND** 旧 ASR 线程 SHALL 在 50ms 内退出; 之后新 `VoiceManager` 接管麦克风输入

## 边界 / 不变量

- **不变量 1**: 音量广播频率由服务端 `frame_interval` 决定 (50ms / 20Hz), 不开新后台 task.
- **不变量 2**: `running` 标志只用于 ASR 线程的退出, 不影响 cpal Stream 的正常 Drop 链路.
- **不变量 3**: 取消信号触发后, `VoiceManager` 本身继续存活直至最后一次 `Arc` 释放; 不需要新 API 来"等线程退出".
- **不变量 4**: 协议层不动 `ClientMessage`; 设备状态页渲染层不动.
