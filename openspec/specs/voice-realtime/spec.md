# voice-realtime Specification

## Purpose
TBD - created by archiving change add-voice-realtime. Update Purpose after archive.
## Requirements
### Requirement: 协议支持音量广播
系统 SHALL 提供一个 `ServerEvent::Volume { value: i32 }` 变体, 承载当前麦克风输入的归一化峰值音量 (0..=100). 客户端不需要单独请求; 字段值在 voice 不可用时为 0.

音量 `value` SHALL 通过 dB 对数刻度从 cpal f32 峰值样本映射得出: 取一个音频回调窗口内样本绝对值的最大值作为 `peak` (0.0..=1.0), 计算 `db = 20 * log10(peak)`, 再线性映射 `[-40 dB, 0 dB]` 到 `[0, 100]`, 超出范围 SHALL 被 clamp. `-40 dB` 是常见麦克风本底噪声量级, 低于此值 SHALL 输出 0. 这样小声音量 (peak 0.05–0.3) 在音量条上能稳定覆盖 30–70 的可见范围, 不退化成 1–3 格. 系统 SHALL 同时对音量做慢速指数衰减 (每 32ms ×0.95), 视觉上呈 VU 表响应特性.

#### Scenario: 服务端广播音量
- **WHEN** 服务端持有 `Arc<VoiceManager>` 且 cpal 输入流在产生数据
- **THEN** 服务端 SHALL 每 50ms (20Hz) 通过 `event_tx` 广播一次 `ServerEvent::Volume`, `value` 等于 `voice.volume().load(Relaxed)`
- **AND** voice 不可用 (`state.voice` 为 `None`) 时, 广播的 `value` SHALL 为 0
- **AND** `value` SHALL 由 dB 映射函数 `peak_to_volume(peak)` 计算得出, 不使用线性 `peak*100`

#### Scenario: 客户端接收并缓存
- **WHEN** 客户端 `apply_event` 收到 `ServerEvent::Volume { value }`
- **THEN** 客户端 SHALL 将 `server.volume` 字段更新为 `value`
- **AND** 现有 `device_status.rs` 渲染逻辑 SHALL 无需改动即可看到音量条随说话变化

#### Scenario: dB 映射函数行为
- **WHEN** cpal 回调读到静音 (peak = 0.0)
- **THEN** `peak_to_volume` SHALL 返回 0
- **WHEN** cpal 回调读到小声音量 (peak = 0.05, ≈ -26 dB)
- **THEN** `peak_to_volume` SHALL 返回 30..=40 之间的整数值
- **WHEN** cpal 回调读到中等音量 (peak = 0.1, -20 dB)
- **THEN** `peak_to_volume` SHALL 返回 50..=55 之间的整数值
- **WHEN** cpal 回调读到满刻度 (peak = 1.0, 0 dB)
- **THEN** `peak_to_volume` SHALL 返回 100
- **WHEN** cpal 回调读到超过 1.0 的异常值
- **THEN** `peak_to_volume` SHALL clamp 到 100

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
`SharedState::rebuild_voice` SHALL 在替换 `self.voice` 之前, 把旧实例的 `running` 标志置 false; 新实例 SHALL 在旧实例退出窗口 (≥ 60ms, 即 50ms `recv_timeout` + 10ms 余量) 之后再覆盖. 这样能保证:

1. 旧 cpal Stream 不会继续向 mpsc 写数据
2. 旧 ASR 线程不会继续占用 sherpa-onnx 解码资源
3. 系统中同时最多只有一个 ASR 实例在跑

#### Scenario: 切换麦克风时旧 ASR 线程退出
- **WHEN** 客户端发送 `SetConfig` 且 `config.speech_name` 与旧值不同
- **THEN** 服务端 `set_config` SHALL 调用 `rebuild_voice`
- **AND** `rebuild_voice` SHALL 先把旧 `VoiceManager` 的 `running` 置 false, 然后 sleep 至少 60ms, 再 `lock+replace`
- **AND** 旧 ASR 线程 SHALL 在 50ms 内退出; 之后新 `VoiceManager` 接管麦克风输入

### Requirement: TTS 流式播放的 cpal Stream 必须 RAII

`TtsPlayer::start_streaming` 返回的 `StreamPlayerHandle` SHALL 持有 cpal `OutputStream`, 通过 RAII 包装 (例如 `OwnedOutputStream`) 在 `StreamPlayerHandle` Drop 时调用 `stream.pause()` 并让 cpal 自己释放底层 device 句柄.

实现 SHALL NOT 使用 `std::mem::forget`, `Box::leak`, 全局单例 (`lazy_static!` / `OnceLock<Stream>`) 或其它"让 cpal Stream 脱离所有权链"的手段来绕过 Drop.

#### Scenario: 流式 TTS 播放完成, device 句柄释放
- **WHEN** `speak_streaming` 调 `start_streaming`, 拿到 `StreamPlayerHandle`, 等到 `handle.is_done()` 返回 true
- **THEN** 调用方继续持有 handle 不再使用
- **AND** handle 离开作用域时 Drop 触发, `OwnedOutputStream::drop` 调 `stream.pause()`, cpal 释放底层 device 句柄

#### Scenario: 流式 TTS 播放过程中触发 rebuild_voice
- **WHEN** `TtsPlayer` 当前正在 `start_streaming` 返回的 handle 持有 cpal Stream, 同时 `SharedState::rebuild_voice` 被调用 (例如用户切音频设备)
- **THEN** 旧 `VoiceManager` 通过 `Arc::drop` 链释放旧 `TtsPlayer` 路径下所有持有 handle 的引用 (例如未来扩展中 `VoiceManager` 持有 current `Option<StreamPlayerHandle>`)
- **AND** 旧 device 句柄 SHALL 被释放, 不残留 leaked stream

> **NOTE**: 当前 `VoiceManager::speak_streaming` 在 `tokio::task::spawn_blocking` 里阻塞等 `is_done()`. 边界情况下 `speak_streaming` 在播放期间被卡住, 旧 device 句柄要等 TTS 自然结束才释放. 这是已知限制, 不在本 spec 范围内承诺"切设备立即打断 TTS".

### Requirement: TtsPlayer::play 基于样本计数判断完成

`TtsPlayer::play` SHALL 通过 cpal OutputStream 回调里累计已播放样本数 (例如 `Arc<AtomicUsize>`), 在主线程循环等待该计数器达到 `audio.samples.len()` 后返回. 实现 SHALL NOT 使用 `std::thread::sleep(wall_clock_duration)` 估算播放时长.

#### Scenario: 正常播放完成
- **WHEN** `TtsPlayer::play` 被调用, cpal callback 累计播放样本数达到 `total = audio.samples.len()`
- **THEN** 主线程的等待循环退出, `play` 函数返回 `Ok(())`

#### Scenario: 设备采样率与 audio.sample_rate 不一致
- **WHEN** 输出设备的实际采样率与 `audio.sample_rate` 不同 (例如需要 resample)
- **THEN** 主线程 SHALL 等到 callback 真正写完 `total` 个样本后才返回, 而不是按 wall-clock 提前返回

#### Scenario: 输出设备卡顿, callback 慢
- **WHEN** cpal callback 累计样本数上升缓慢 (例如 OS 调度抖动)
- **THEN** 主线程 SHALL 继续等待, 不提前返回截断音频

### Requirement: TtsHandler 不允许手写 unsafe Send/Sync impl

`TtsHandler` 的线程安全 SHALL 完全由内部 `Arc<Mutex<OfflineTts>>` 提供. 实现 SHALL NOT 显式声明 `unsafe impl Send for TtsHandler` 或 `unsafe impl Sync for TtsHandler`. `Arc<Mutex<T>>` 在 `T: Send` 时天然是 `Send + Sync`, 删除 unsafe impl 不会破坏现有跨线程使用.

#### Scenario: 删 unsafe impl 后, TtsHandler 仍可跨线程
- **WHEN** `TtsHandler` 上的两个 `unsafe impl` 被删除
- **THEN** `cargo check --all-features --all-targets` SHALL 仍然通过
- **AND** `ws::handle_command` 里 `tokio::task::spawn_blocking(move || voice.speak_streaming(...))` 的闭包跨线程捕获 `voice: Arc<VoiceManager>` (其中含 `TtsHandler`) SHALL 编译通过

#### Scenario: 未来给 TtsHandler 加非 Mutex 保护的字段
- **WHEN** 有人在 `TtsHandler` 上加一个非 `Mutex` 保护的字段 (例如裸的 `Rc<i32>` 或 `*mut T`)
- **THEN** 编译器 SHALL 报错 `TtsHandler cannot be sent between threads safely` 或类似 trait bound 错误, 强制开发者重新考虑同步方案
- **AND** 实现 SHALL NOT 用"加回 unsafe impl"绕过这个错误

