## Why

`crates/ele_bot_server/src/media/voice/tts.rs` 在调研"前后端实时切换音频设备"路径时被发现存在几处真实/潜在问题: 流式 TTS 通过 `std::mem::forget` 泄漏 cpal Stream, 与热重建 `rebuild_voice` 路径冲突; 非流式 `play()` 用 `thread::sleep` 等播放完成, 易受采样率/buffer 影响截音; `unsafe impl Send/Sync` 是冗余的危险代码. 同时 `voice-realtime` spec 缺少 TTS 播放生命周期的要求, 后续维护者容易踩同样的坑.

## What Changes

- **`TtsPlayer::start_streaming`**: 去掉 `mem::forget(stream)`. 把 cpal Stream 绑进返回的 `StreamPlayerHandle` (或一个 RAII 守卫), 让调用方持有到播放结束再 drop. 这样 `rebuild_voice` drop 旧 `TtsPlayer` 时能连带释放旧 device 句柄.
- **`TtsPlayer::play`**: 把 `thread::sleep(duration_ms + 100)` 替换为 cpal OutputStream 的回调里计数已播放样本, 主线程通过 `Arc<AtomicUsize>` 或一次性 channel 等到位后返回. 同时移除无用的 `is_playing: AtomicBool`.
- **`TtsHandler`**: 删掉 `unsafe impl Send` / `unsafe impl Sync`. `Arc<Mutex<OfflineTts>>` 天然 Send+Sync, 不需要手写 unsafe impl.
- **扩展 `voice-realtime` spec**: 新增 Requirements 覆盖 TTS 播放生命周期 (流式路径的所有权, 非流式的完成信号, 重建时旧 TTS 线程/stream 的退出窗口).
- **`docs/voice-hot-swap.md`**: 记录 `rebuild_voice` 的设计意图: 60ms sleep 的来源, 为什么 drop old Arc, TTS streaming 路径与热重建的交互 (避免后续把 sleep 去掉或漏考虑 stream 所有权).

## 目标

- 让 `rebuild_voice` 在用户切音频设备后能真正释放旧 cpal Stream + 旧 device 句柄, 不靠"用户切设备时不播流式 TTS"这种隐性约束.
- 让 `TtsPlayer::play` 的播放完成判断不再依赖 wall-clock, 而是看真实样本流.
- 把 `TtsHandler` 的线程安全责任完全交给 `Mutex`, 不留"我自己保证"的误导信号.
- 把热重建与 TTS 播放的设计意图落到 spec + docs, 防止回归.

## 非目标 (Non-goals)

- **不**改 streaming TTS 的协议层 (WebSocket 增量推 audio chunk). 当前是先生成完整 samples 再播, 改 streaming 协议是另一个事.
- **不**换 sherpa-onnx VITS 模型 / 不优化音色.
- **不**改 ASR 部分 (`asr.rs` 的取消走的是 `running: Arc<AtomicBool>`, 与本 change 无关).
- **不**写跨进程的 e2e 测试验证 device handle 释放 — 验证仅限设计层面 + 静态分析.

## Capabilities

### New Capabilities
- 无新增 capability. TTS 播放生命周期归到现有的 `voice-realtime` 能力下, 不另起一份 spec.

### Modified Capabilities
- `voice-realtime`: 新增 Requirements 覆盖 TTS 播放生命周期 (流式 Stream 所有权、play 完成信号). 现有 ASR/音量/取消的 Requirements 保持不变.

## Impact

- 受影响 crate: `crates/ele_bot_server` (改 `tts.rs` + `state.rs` 注释)
- 受影响文件:
  - `crates/ele_bot_server/src/media/voice/tts.rs` — 改 `TtsPlayer::start_streaming`, `TtsPlayer::play`, 删 `unsafe impl`
  - `crates/ele_bot_server/src/state.rs` — 加强 `rebuild_voice` 注释, 说明 TTS streaming 路径的交互
  - `openspec/specs/voice-realtime/spec.md` — 加 delta spec
  - `docs/voice-hot-swap.md` — 新建
- 不影响: `crates/ele_bot_proto` / `crates/ele_bot_client` / 协议层
- 不引入新依赖