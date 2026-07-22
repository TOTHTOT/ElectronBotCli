## Context

`crates/ele_bot_server/src/media/voice/tts.rs` 当前实现里, `TtsPlayer` 持有 `cpal::Device`, 但具体到 cpal `Stream` 时:

- **`play()`** 把 `Stream` 留在栈上, 用 `thread::sleep(duration_ms + 100)` 假设 wall-clock 跟音频设备时钟同步
- **`start_streaming()`** 调 `std::mem::forget(stream)`, 让 Stream 脱离所有权链, 只靠 `playback_done: AtomicBool` 让调用方知道"完了"

上游 `SharedState::rebuild_voice` 的设计意图是: 用户切音频设备时, drop 旧 `VoiceManager` → drop 旧 `TtsPlayer` → 旧 cpal `Device` 句柄释放. 但 `mem::forget` 让 streaming 路径下的 Stream + 旧 device 句柄泄漏, 与热重建冲突.

另外 `TtsHandler` 上写的 `unsafe impl Send + Sync` 是冗余的误导信号 —— `Arc<Mutex<OfflineTts>>` 天然是 Send+Sync.

## Goals / Non-Goals

**Goals:**
- 让 `start_streaming` 的 cpal Stream 有明确的所有者, 跟着 `TtsPlayer` 的 Drop 走
- 让 `play()` 的完成判断看真实样本流, 不看 wall-clock
- 让 `TtsHandler` 的线程安全完全交给 `Mutex`, 删 unsafe impl
- 把热重建 + TTS 播放的设计意图写到 `docs/voice-hot-swap.md`, 防回归

**Non-Goals:**
- 不改 WebSocket 协议层
- 不改 ASR / 音量 / 取消信号的现有逻辑
- 不优化 TTS 模型加载 / 推理速度
- 不写跨进程 e2e 测试

## Decisions

### 决策 1: `start_streaming` 用 RAII 守卫持有 Stream

**做法**: 引入一个新类型 `OwnedOutputStream(cpal::Stream)`, 实现 `Drop` 时调 `stream.pause()` 然后让 cpal 自己清理 (cpal 5.x 的 `Stream` 已经 `Send`+ 内置 drop 释放). 把 `OwnedOutputStream` 放进 `StreamPlayerHandle` 的字段里, 跟 buffer/synthesis_done/playback_done 并列.

```rust,ignore
/// cpal OutputStream 的 RAII 包装 — Drop 时停流并释放 device 句柄
pub struct OwnedOutputStream {
    stream: cpal::Stream,
}

impl Drop for OwnedOutputStream {
    fn drop(&mut self) {
        // cpal Stream 的 Drop 会自然调 pause + 释放, 这里显式 pause
        // 是为了在 rebuild_voice drop 时让旧设备更快可用
        let _ = self.stream.pause();
    }
}

pub struct StreamPlayerHandle {
    buffer: Arc<Mutex<Vec<f32>>>,
    synthesis_done: Arc<AtomicBool>,
    playback_done: Arc<AtomicBool>,
    /// 持有 cpal Stream, Drop 时停流. 调用方必须持有到播放结束.
    _stream: OwnedOutputStream,
}

impl TtsPlayer {
    pub fn start_streaming(&self, sample_rate: u32) -> Result<StreamPlayerHandle> {
        // ... 同前
        let stream = self.device.build_output_stream(...)?;
        stream.play()?;
        Ok(StreamPlayerHandle {
            buffer,
            synthesis_done,
            playback_done,
            _stream: OwnedOutputStream { stream },
            // ↑ 不再 mem::forget
        })
    }
}
```

**调用方契约**: `VoiceManager::speak_streaming` 当前是 `let handle = ...; spawn 合成线程; while !handle.is_done()`. 这个调用方对 `handle` 的所有权已经覆盖整个播放过程, 所以 RAII 守卫跟调用方寿命对齐 —— 无需改 `speak_streaming`.

**被考虑的替代方案**:
- **A. 把 Stream 绑到 `TtsPlayer` 字段里, 计数已播放 → 播完自动 drop**: 需要 `TtsPlayer` 是线程间共享 (`Arc<Mutex<TtsPlayer>>`), 而当前 `TtsPlayer` 是 `Option<TtsPlayer>` 在 `VoiceManager` 里. 改这个会影响 VoiceManager 整体架构, 超出本 change 范围. 弃.
- **B. 用 `mpsc::Sender<()>` 通知 cpal callback 自己停**: cpal callback 是 `FnMut`, 不能在里面消费 `Receiver`. 弃.
- **C. `Box::leak` 然后通过 `Arc<Weak>` 让 handle 持有, 结束时显式 `unsafe { drop(...) }`**: 引入 `unsafe` 代码, 跟本 change "去 unsafe" 的目标冲突. 弃.

### 决策 2: `play()` 用回调里计数样本, 主线程等位置信号

**做法**: cpal OutputCallbackInfo 没有"buffer 已经写完"的直接信号, 但回调闭包可以记录"已经写过多少个样本". 把已写样本数放到 `Arc<AtomicUsize>`, 主线程在循环里 `while total != played { park_timeout(20ms) }`. 这跟 `start_streaming` 用的 `Arc<AtomicBool>` 是同一套思路, 复用 `OwnedOutputStream` 包装.

```rust,ignore
pub fn play(&self, audio: &TtsAudio) -> Result<()> {
    let total = audio.samples.len();
    let played = Arc::new(AtomicUsize::new(0));
    let played_clone = played.clone();

    let config = cpal::StreamConfig {
        channels: audio.channels,
        sample_rate: audio.sample_rate,
        buffer_size: cpal::BufferSize::Default,
    };

    let stream = self.device.build_output_stream(
        &config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            // 写 data, 同时累加 played_clone.fetch_add(data.len(), Relaxed)
            ...
        },
        |err| log::error!("TTS 流错误: {}", err),
        None,
    )?;

    stream.play()?;

    // 阻塞直到 callback 把所有样本都写过
    while played.load(Relaxed) < total {
        std::thread::sleep(Duration::from_millis(10));
    }
    // Stream 在这里自然 drop (因为 `stream` 在栈上, 函数返回时 drop)
    Ok(())
}
```

**为什么不用 `park_timeout`**: cpal 回调是另一个 OS 线程跑的, 没法直接 park 主线程等 callback 唤醒. 用 `sleep(10ms)` 简单可靠, 跟现有 `start_streaming` 的轮询模式一致.

**为什么不用 channel**: cpal callback 是 `FnMut`, 不能捕获 `mpsc::Sender` 拿走 (每次回调都要 send). 用 `AtomicUsize` 更直接.

**被考虑的替代方案**:
- **A. `cpal::Stream::pause` 在 callback 里检测 `played == total` 后自动停**: cpal 5.x 的 callback 签名不允许在内部 pause 自己 (要 `&mut self`, 而 callback 是 `FnMut` 闭包). 弃.
- **B. 用 `OutputCallbackInfo.timestamp()` 算 wall-clock**: cpal timestamp 是设备时钟, 不是系统时钟. 算出来仍然是相对时间, 不解决根本问题. 弃.

### 决策 3: 删 `unsafe impl Send + Sync`

**做法**: 删 `tts.rs` line 30-31 两个 unsafe impl. `Arc<Mutex<OfflineTts>>` 的 `Send`/`Sync` 由 `Mutex<T>: Send + Sync where T: Send` 自动派生. `i32` 也自动 Send+Sync. 整个 `TtsHandler` 天然满足 trait bound.

**验证**: 删除后 `cargo check` 应该 0 错 (Mutex 仍然提供同步). 如果有调用方显式依赖 `unsafe impl`, 会编不过 —— 这种情况下 unsafe impl 是在掩盖一个真正的 Send/Sync 问题, 应该显式解决.

**被考虑的替代方案**:
- **A. 保留 unsafe impl + 加 SAFETY 注释解释为什么**: 跟"删掉它"目标冲突, 也不解决"未来加字段会静默破坏"的隐患. 弃.

### 决策 4: TTS 线程的"取消信号"模型

TTS 路径不像 ASR 那样有"长跑线程"需要 cancel —— `speak()` / `speak_streaming()` 是阻塞调用, 在 `spawn_blocking` 里跑. 热重建不需要等 TTS 线程退出. 但**如果用户在播流式 TTS 时切设备**, 当前 `speak_streaming` 还在阻塞 `while !handle.is_done()`. 新的 `OwnedOutputStream` 让 `handle` Drop 时 stream 停, 但 `playback_done` 怎么被 set?

**做法**: `OwnedOutputStream` 在 Drop 时显式 `pause()`, 但**不**设 `playback_done`. 调用方 `VoiceManager::speak_streaming` 的 `while !handle.is_done()` 循环变成"等播放自然结束"或者"等 rebuild_voice drop handle". 后者要求调用方不持有 handle.

实际问题: `speak_streaming` 持有 `handle` 等 `is_done()`. 如果用户在播期间触发 `rebuild_voice`, 旧 `VoiceManager` 被 drop (包括 `TtsPlayer` 和正在用的 `StreamPlayerHandle`), `speak_streaming` 拿到的 `handle` 是 clone 出来的 Arc, 仍然存活. 这个 Arc 没人 drop, `OwnedOutputStream` 也不会被 drop.

**结论**: 这个边界情况不解决也行 (speak_streaming 自然结束后 handle 就 drop 了, 旧 device 句柄也跟着释放). 当前 `speak_streaming` 阻塞在 `spawn_blocking` 任务里, 用户感知是"TTS 播完后才切设备". 符合"切设备不阻塞使用"的要求.

如果以后要支持"切设备立即打断 TTS", 需要在 `rebuild_voice` 里能拿到当前播放中的 `StreamPlayerHandle` 并设 `playback_done`. 那是另一个 change.

**被考虑的替代方案**:
- **A. 在 `VoiceManager` 里加 `current_playback: Mutex<Option<Arc<StreamPlayerHandle>>>`, `rebuild_voice` 时设 done**: 引入 state 复杂度, 而且 `speak_streaming` 在 spawn_blocking 里同步持有 handle, Mutex 不解决问题. 弃 (留给未来 change).

### 决策 5: spec delta 位置

**做法**: 在 `openspec/changes/harden-tts-and-document-voice-hotswap/specs/voice-realtime/spec.md` 写 delta (用 `## ADDED Requirements` / `## MODIFIED Requirements` 章节). 不动 `openspec/specs/voice-realtime/spec.md` (源 spec 在 change archive 时再合).

**delta 包含什么**:
1. **ADDED Requirement**: TTS 流式播放的 cpal Stream 必须在 `StreamPlayerHandle` 的 Drop 时释放 (不允许 `mem::forget` / `Box::leak` / 全局单例)
2. **ADDED Requirement**: `TtsPlayer::play` 必须基于已播放样本数判断完成, 不允许 `thread::sleep(wall_clock_duration)`
3. **ADDED Requirement**: `TtsHandler` 的线程安全仅依赖内部 `Mutex`, 不允许手写 `unsafe impl Send`/`unsafe impl Sync`
4. **MODIFIED Requirement**: 把现有 `voice-realtime` 的 "rebuild 窗口" 从 50ms 改成 "至少 50ms (当前实现 60ms, 含 10ms 余量)", 跟实际代码对齐

### 决策 6: `docs/voice-hot-swap.md` 内容大纲

新建 `docs/voice-hot-swap.md`, 给后来人解释为什么:
- 为什么 `rebuild_voice` 不用 `tokio::spawn` 或 `JoinHandle`
- 为什么 60ms sleep (不是 50ms, 不是 100ms)
- 为什么 drop old Arc 而不是 abort 旧 ASR 线程
- 为什么 TTS 路径不在 hot-swap 范围 (TTS 是阻塞 spawn_blocking)
- 为什么 TTS streaming 的 cpal Stream 必须 RAII

## Risks / Trade-offs

**[Risk]** `OwnedOutputStream::drop` 调 `stream.pause()` 可能阻塞 (Windows 上 WASAPI 共享模式对独占设备的暂停有几十毫秒延迟)
→ **Mitigation**: 把 `pause()` 放在 Drop 里, 用 `let _ = self.stream.pause()` 吞错. 即使 pause 慢, 也只在 `rebuild_voice` 触发时发生一次, 不影响正常播放路径.

**[Risk]** `play()` 的 `while played.load() < total { sleep(10ms) }` 在设备卡顿时可能 busy-loop (理论上不会, sleep 10ms 已经节流)
→ **Mitigation**: 接受这个开销. 它跟现有 `start_streaming` 的 `while !is_done() { sleep(50ms) }` 是同一思路. 极端卡顿场景下两种实现都没救.

**[Risk]** 删 `unsafe impl Send/Sync` 后, 如果 sherpa-onnx 升级到 `OfflineTts: !Send`, 当前代码就编不过
→ **Mitigation**: sherpa-onnx 1.12.x 的 `OfflineTts` 是 Send 的, 这一点短时间不会变. 即使真变了, 也是显式 Send 错误, 强制我们思考同步方案, 不会再有"unsafe impl 掩盖问题"的情况.

**[Trade-off]** 不写跨进程 e2e 测试验证 device handle 释放
→ 接受. cpal Stream 的 device 句柄释放验证需要写系统级测试 (监听 device 拔插事件 / 持有计数), 跨平台差异大. 当前阶段的验证:
  1. 静态分析: `mem::forget` / `Box::leak` 已用 clippy lints (`mem_forget`, `box_leak`) 检查
  2. 设计论证: `OwnedOutputStream` 的 Drop 路径明确, RAII 标准
  3. 代码 review: 调用方 (speak_streaming) 持有 handle 到 `is_done()`, Drop 路径覆盖

**[Risk]** `speak_streaming` 阻塞期间切设备, 旧 device 句柄要等 TTS 播完才释放 (不像 ASR 是 cancel signal 即时停)
→ 用户感知: TTS 播完后才能感知设备切换. 对话场景 OK (LLM 回话不会太长), 长文本 TTS 可能感知延迟. 在 spec delta 里**不**承诺"切设备立即打断 TTS", 把这个边界留给未来 change.

## Open Questions

- sherpa-onnx 的 `OfflineTts::generate_with_config` 在 `Mutex` 锁持有期间, callback 里能不能**重入**调 `TtsHandler::synthesize`? 如果不能, 流式路径目前是"先 release 锁, 然后 callback 触发后再次 lock" 的递归模式, 没问题. 但如果未来想"边生成边释放锁", 需要确认. **暂不解决, 仅在 code review 时确认.**
- `cpal::Stream` 的 `pause()` 在 Windows WASAPI 共享模式下的延迟数据 —— 没有公开 benchmark, 用 `_ = ...` 吞错作为兜底.