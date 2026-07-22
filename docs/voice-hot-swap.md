# Voice Hot-Swap 设计说明

> 写给后续维护这个项目的人. 解释 `SharedState::rebuild_voice` 的设计理由,
> 以及它和 TTS 播放路径的交互. 代码改了之后, 这里的 rationale 可能过时,
> 记得同步更新.

## 背景

服务端在用户切换输入/输出设备时, 不能重启整个进程 —— 用户已经在跟机器人对话,
重启会丢状态. 我们的方案是**热重建** `VoiceManager`: 用新设备构造一个新实例,
替换 `SharedState.voice`, 旧实例通过 `Arc::drop` 自然释放.

这套机制依赖几个关键设计决策, 后面一一解释.

## 关键决策

### 1. 为什么 `rebuild_voice` 不用 `tokio::spawn` 或 `JoinHandle`

旧 ASR 线程 (`asr::recognition_thread`) 是普通 `std::thread::spawn` 起的, 已经在
我们的 `running: Arc<AtomicBool>` 控制下. 我们**没有**存 `JoinHandle`, 也不需要 join:

- 线程退出靠 `running.store(false)` 通知, 然后它自己 `recv_timeout` wake 后检查标志退出
- 旧实例的 cpal Stream / sherpa-onnx 解码器都绑在 `Arc<VoiceManager>` 上, 引用计数归零时
  Rust 自动 drop
- 如果强行 `thread::JoinHandle::join().unwrap()` 阻塞等, 切设备会有 50ms 延迟可见;
  如果 `join` 失败 (panic 在线程里) 会污染 caller

所以这里**故意不存 JoinHandle**, 让所有资源跟随 `Arc<VoiceManager>` 的引用计数.

### 2. 为什么 `sleep(60ms)` 不能去掉或改短

```rust
old.running().store(false, Ordering::Relaxed);
std::thread::sleep(Duration::from_millis(60));  // ← 这一行
let new_voice = Self::init_voice(&config)?;
```

`asr::recognition_thread` 内部是这种循环:

```rust
loop {
    match audio_rx.recv_timeout(Duration::from_millis(50)) {
        Ok(samples) => { /* 解码 */ }
        Err(RecvTimeoutError::Timeout) => {
            if !running.load(Ordering::Relaxed) {
                break;  // ← 退出点
            }
        }
        Err(RecvTimeoutError::Disconnected) => break,
    }
}
```

**只有** `recv_timeout` 超时 wake 后, 才会检查 `running` 标志. 所以从 `store(false)`
到线程真正退出, 最坏情况是 `50ms` (整次 recv_timeout 还没超时) + 一点调度延迟.

`60ms = 50ms + 10ms` 余量是经验值. 如果把这个 sleep 改成 `0ms`, 在 Windows WASAPI
上会出现 `Failed to bind audio device` 间歇性失败 —— 旧线程还没让出设备的独占锁,
新 cpal Stream 已经尝试 open, 设备被独占占用.

**不要**优化掉. **不要**改短除非有充分 benchmark.

### 3. 为什么 drop old Arc 而不是 abort 旧 ASR 线程

Rust 标准库没有 `std::thread::Thread::abort`. 即便用 `pthread_cancel` 之类的
平台 API, sherpa-onnx 内部持有 C++ 对象 + 内存映射模型, 强杀可能让后续重建
时内存锁死 (Sherpa-onnx 已经加载的模型文件不能 unmap).

`running` + 自然退出是 sherpa-onnx 文档推荐的清理方式. 配合 `Arc::drop` 触发
cpal Stream Drop, 整套清理是 Rust 友好的, 没有 unsafe.

### 4. TTS 路径为什么不在热重建里 cancel

ASR 是"长跑线程" — 一直跑, 一直消耗麦克风, 切设备必须让它退. 所以有 `running`
标志.

TTS 是**按需调用** — `VoiceManager::speak` / `speak_streaming` 是阻塞调用,
跑在 `tokio::task::spawn_blocking` 里 (见 `ws.rs::handle_command::TtsSpeak`).

切设备时, 旧 TTS 调用可能还在跑 (例如用户说了一句, LLM 回了一句长文本, TTS 还在
播前半句). 那个 spawn_blocking 闭包持有了 `Arc<VoiceManager>`, 所以 `rebuild_voice`
里的 `take()` 拿走的 Arc 引用计数**不会归零** (还有其他引用).

后果:

- 旧 TTS 仍在跑, 旧 cpal Stream 还在出声
- 旧 device 句柄要等 TTS 自然播完才释放
- 新设备已经接管 (新 VoiceManager 在跑), 但旧 TTS 仍在用旧设备发声
- 用户感知: "切完设备后, 还有一句话从旧设备里冒出来"

这是已知设计取舍. 修复路径 (留待未来 change):

1. `VoiceManager` 加 `current_playback: Mutex<Option<Arc<StreamPlayerHandle>>>`
2. `speak_streaming` 进入时注册, 退出时清掉
3. `rebuild_voice` 拿到这个 handle, 调 `mark_synthesis_done` 或类似 API 强制
   旧 callback 退出 + Drop StreamPlayerHandle → Drop OwnedOutputStream → 旧设备释放

这个方案设计阶段就**没**纳入本 change, 因为它会引入:
- `VoiceManager` 内部状态复杂度 (新增 Mutex + register/deregister)
- 需要客户端发 "打断 TTS" 协议 (`ClientMessage::InterruptTts`?) 还是服务端自动检测
- 跟 `speak_blocking` 的 cancel 安全 (sherpa-onnx 中断是否能干净退出?)

不解决也行: 用户场景里 TTS 通常 1-3 秒, 切设备延迟可接受. 留给未来.

### 5. TTS streaming 的 cpal Stream 必须 RAII

`TtsPlayer::start_streaming` **禁止**用 `mem::forget` / `Box::leak` / 全局单例
把 cpal Stream 脱离所有权链. 必须放进 `StreamPlayerHandle._stream` 字段, 跟随
Drop 释放.

原因:

- `start_streaming` 在 cpal callback 里消费共享 buffer 推音频
- callback 需要 stream 保持存活
- 如果 stream 被 forget, 调用方拿到 handle 但 stream 不归 handle 管
- `rebuild_voice` drop 旧 `TtsPlayer` 时, stream 还在跑 (持有旧 device 句柄),
  旧 device 不释放

修复方式: `OwnedOutputStream` RAII 包装, `Drop` 调 `stream.pause()`. 见
`crates/ele_bot_server/src/media/voice/tts.rs` 顶部模块注释.

## 流程图

```text
用户切设备 (客户端 picker 提交 SetConfig)
   ↓ WS
ServerConfig::set_config 检测 audio_changed
   ↓
rebuild_voice
   ├─ take() 旧 Arc → 旧 Arc 在本函数栈上
   ├─ old.running.store(false)          ← 通知旧 ASR 线程退
   ├─ sleep(60ms)                        ← 等旧 ASR wake + 退出 + 让出设备
   ├─ init_voice(cfg) → new VoiceManager
   ├─ self.voice = Some(Arc::new(new))   ← 新实例接管
   └─ drop(old)                          ← 旧 Arc 引用计数归零 → Drop 旧 Stream + ASR 资源
```

## 已知边界

| 场景 | 当前行为 | 期望行为 (未来 change) |
|---|---|---|
| 切设备时 TTS 正在播 | 旧 TTS 跑完才释放旧设备 | 立即打断, 强制 Drop |
| ASR running=false 后旧线程卡死 | sleep(60ms) 后强制 init_voice 接管 | 加超时 panic 或 send error 后回退旧实例 |
| 两次切设备间隔 < 60ms | 旧 sleep 未完成 → 新 sleep 累积 | 用 `AtomicU64` 单调时钟, 跳过重叠窗口 |

## 修改 checklist

改动这一块代码时, 请检查:

- [ ] 没去掉 `sleep(60ms)`
- [ ] 没把 `mem::forget` / `Box::leak` 加回 `start_streaming`
- [ ] `TtsHandler` 上没加回 `unsafe impl Send/Sync`
- [ ] `OwnedOutputStream` 还在 `StreamPlayerHandle` 字段里
- [ ] 没改 `asr::recognition_thread` 的 `recv_timeout` 长度 (改了就同步改 sleep)