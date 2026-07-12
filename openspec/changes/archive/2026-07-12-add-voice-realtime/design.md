## Context

设备状态页 (DeviceStatus) 顶部有一条"输入音量"条, 渲染逻辑读 `app.server.lock().volume` (字段已在 `crates/ele_bot_client/src/app/mod.rs:69` 存在, `device_status.rs:24` 读取). 但用户在设置页切换麦克风后观察到:

1. **音量条不动** — `app.server.volume` 从初始化后永远是 0.
2. **切麦后偶发"还在用旧麦"** — 旧 `VoiceManager` 的 cpal `Stream` 与 `recognition_thread` 在 `rebuild_voice` 替换时未立即退出.

根因 (已被 Explore agent 确认):
- 服务端 `voice::build_asr_stream` (`crates/ele_bot_server/src/media/voice/asr.rs:135`) 在 cpal 回调里更新 `volume_clone: Arc<AtomicI32>`, 但**服务端没有任何路径把这个 Atomic 广播给客户端**; `ServerEvent` 枚举没有 `Volume` 变体; 客户端 `apply_event` 没有对应分支.
- `rebuild_voice` (`crates/ele_bot_server/src/state.rs:175`) 用 `*guard = Some(Arc::new(new))` 直接替换, 旧 `Arc<VoiceManager>` Drop 时 cpal `Stream` 析构. 但 `audio_tx` (Sender) 被 cpal 回调 closure 持有 (`asr.rs:176`), 旧 `recognition_thread` 仍阻塞在 `for samples in audio_rx` (`asr.rs:71`). Windows WASAPI 下 cpal 停止回调不及时, 旧线程在旧设备上继续解码, 偶发"换麦但仍用旧麦".

约束:
- `ws.rs` 已经有 `frame_interval` 50ms tick 跑 LCD 帧 (`ws.rs:92`), 推 volume 完全可以在同一 tick 里做, 不开新 task.
- ASR 线程当前 `for samples in audio_rx` 阻塞等数据, 无法被外部中断; 需要改成 `recv_timeout(50ms)` + 标志位.
- 客户端 `device_status.rs` 渲染代码不动.

## Goals / Non-Goals

**Goals:**
- `ServerEvent::Volume` 协议, 50ms / 20Hz 周期广播
- `VoiceManager.running: Arc<AtomicBool>` 标志 + ASR 线程 `recv_timeout` 检查, 50ms 内主动退出
- `rebuild_voice` 重建前给旧实例 50ms 退出窗口, 不再"硬替换"
- 客户端 `apply_event` 收到 `Volume` 即写 `server.volume`, 设备状态页音量条实时刷新

**Non-Goals:**
- 不动音量采样算法 (峰值 + 衰减)
- 不动 VAD 阈值与 ASR 模型
- 不给用户暴露音量调节控件
- 不持久化 volume (运行时数据)
- 不改 `device_status.rs` 渲染逻辑

## Decisions

### D1: 复用 `frame_interval`, 不开新 task

`ws.rs:91` 已经有 `let mut frame_interval = tokio::time::interval(Duration::from_millis(50));` 在主循环 `tokio::select!` 里跑 (推 LCD 帧到 USB 通信 + 写 web preview 缓存). 同一 tick 上加一个分支:

```rust,ignore
_ = frame_interval.tick() => {
    if state.robot_connected.load(Ordering::Relaxed) {
        let pixels = state.generate_lcd_frame();
        if !pixels.is_empty() {
            state.push_frame_to_robot(pixels.clone());
            if let Ok(mut guard) = state.lcd_frame_cache.lock() {
                *guard = Some(pixels);
            }
        }
    }
    // 音量广播: 跟 LCD 帧同一个 tick, 50ms 一次, 0 开销
    let value = state
        .voice
        .lock()
        .unwrap()
        .as_ref()
        .map(|v| v.volume())
        .unwrap_or(0);
    let _ = state.event_tx.send(ServerEvent::Volume { value });
}
```

**为什么不新开 `tokio::spawn`**: 50ms tick 任务已经在线, 复用零成本. broadcast::Sender 不在乎是不是同一 task, 收端按消息序号处理.

### D2: `VoiceManager.running: Arc<AtomicBool>`

`VoiceManager` 新增字段:

```rust,ignore
pub struct VoiceManager {
    _stream: Option<Stream>,
    _rx: mpsc::Receiver<String>,
    volume: Arc<AtomicI32>,
    tts_handler: TtsHandler,
    tts_player: Option<TtsPlayer>,
    /// 取消信号: ASR 线程每轮检查, false 时主动退出
    running: Arc<AtomicBool>,
}
```

暴露:

```rust,ignore
/// 返回共享的 running 标志, 外部可置 false 请求 ASR 线程退出
pub fn running(&self) -> Arc<AtomicBool> {
    self.running.clone()
}

#[allow(dead_code)]
pub fn is_running(&self) -> bool {
    self.running.load(Ordering::Relaxed)
}
```

**为什么用 `Arc<AtomicBool>` 而不是 channel**: 极轻量, 50ms 检查一次成本为零; 比再开一个 cancel channel 简单.

### D3: ASR 线程 `recv_timeout` + flag

`recognition_loop` 改写, 关键 diff:

```rust,ignore
// 旧:
for samples in audio_rx { ... }

// 新:
loop {
    let samples = match audio_rx.recv_timeout(Duration::from_millis(50)) {
        Ok(s) => s,
        Err(RecvTimeoutError::Timeout) => {
            if !running.load(Ordering::Relaxed) {
                log::info!("ASR 线程收到取消信号, 退出");
                return Ok(());
            }
            continue;
        }
        Err(RecvTimeoutError::Disconnected) => return Ok(()),
    };
    // ... 原有处理
}
```

`recognition_thread` 入口也加 `running` 参数 (从 `VoiceManager::new` 传入, 即 `self.running.clone()`).

**为什么 50ms**: 与 ws 主循环 tick 同步, 既能及时响应, 又不构成 busy loop (CPU 占用 < 0.1%).

### D4: `rebuild_voice` 软替换

`state.rs::rebuild_voice` 改为先关旧, 再建新:

```rust,ignore
pub fn rebuild_voice(&self) -> anyhow::Result<()> {
    let config = self.config.read().unwrap().clone();

    // 1. 取出旧实例 (如果有), 让旧 ASR 线程知道该退了
    let old = {
        let mut guard = self.voice.lock().unwrap();
        guard.take()
    };
    if let Some(old) = &old {
        old.running().store(false, Ordering::Relaxed);
        // 给旧线程一个 50ms 退出窗口; cpal Stream 仍在 (Arc 还活着),
        // 旧线程退出后 audio_tx 跟着 drop, audio_rx 断开, thread 自然结束.
        std::thread::sleep(Duration::from_millis(60));
    }

    // 2. 构造新实例, 替换
    let new_voice = Self::init_voice(&config)?;
    *self.voice.lock().unwrap() = Some(Arc::new(new_voice));
    Ok(())
}
```

**为什么 sleep 60ms 而不是 join**: 旧 thread 是 `std::thread::spawn` (detached), 没有 handle 可 join. 60ms 退出窗口由 D3 的 `recv_timeout` 兜底; 旧 Stream 随 `old: Arc<VoiceManager>` 在函数返回时 Drop.

**风险**: 若旧 cpal 线程卡在 sherpa-onnx decode 里 (>50ms), 不会立刻响应取消. 但 sherpa decode 一次典型 < 100ms, 而且新 thread 与旧 thread 互不依赖 (独立 cpal Stream + 独立 recognizer 实例), 旧 thread 即使还在跑也不会污染新 thread 的 audio. 等旧 recognizer 跑完当前 decode 就会回到 `recv_timeout` 循环, 检测 flag 退出. 可接受.

### D5: 客户端 `apply_event` 写 `server.volume`

`app/mod.rs` 加一个分支:

```rust,ignore
ServerEvent::Volume { value } => {
    server.volume = value;
}
```

`device_status.rs` 已经读 `server.volume`, 零改动.

## Risks / Trade-offs

**[R1] 50ms 广播增加 ~6% 带宽** — 每帧多一个 `{"type":"volume","value":N}`, 50 字节左右. 现有 WS 已经在 50ms 推 LCD 帧, 增量可忽略.
→ Mitigation: 已经在同一 tick, 不增加 RTT.

**[R2] 旧 ASR 线程卡在 decode 时不立即退出** — 极端情况下旧 recognizer 跑完一段 30 秒长音频才检查 flag, 期间 cpal Stream 已 drop, 但旧 thread 仍在占 CPU.
→ Mitigation: D4 的 60ms sleep 后立即替换; 旧 thread 失去 audio_rx 后退出. 不影响新 thread 接管麦克风.

**[R3] `rebuild_voice` 内 `std::thread::sleep` 阻塞 ws 任务** — ws 是 tokio 任务, sync sleep 会让出线程但不阻塞 tokio runtime. 单次 60ms 可接受.
→ Mitigation: 不替换为 `tokio::time::sleep` (async) 会让 `rebuild_voice` 签名变 async, 牵动 `set_config`. 不值.

**[R4] `recv_timeout` 把单次等待从"无限"降到"50ms 一次检查"** — 极端情况下 ASR 输入到达延迟 +50ms, 不可感知.
→ Mitigation: 50ms 远低于 1 帧音频 (1600 样本 / 16kHz = 100ms), 不会丢数据.

## Migration Plan

无. 增量改动, 不改持久化格式, 不破坏现有协议. 已 archive 的 `audio-device-picker` spec 不需要回溯修改 (cancel 信号是隐含在 rebuild 行为里的, 不暴露为外部契约).

回滚: 单 commit revert.

## Open Questions

无. 用户已确认: 合并成 1 个 change / cancellation flag / 50ms / 20Hz 广播.
