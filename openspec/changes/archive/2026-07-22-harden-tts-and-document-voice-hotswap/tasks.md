## 1. TTS 播放路径修复

- [x] 1.1 在 `crates/ele_bot_server/src/media/voice/tts.rs` 引入 `OwnedOutputStream` RAII 包装 (`pub struct OwnedOutputStream { stream: cpal::Stream }`, `Drop` impl 调 `stream.pause()`), 给它补 rustdoc 说明"在 `StreamPlayerHandle` Drop 时释放 cpal Stream + 旧 device 句柄"
- [x] 1.2 改 `TtsPlayer::start_streaming`: 返回的 `StreamPlayerHandle` 加一个 `_stream: OwnedOutputStream` 字段; 删掉 `std::mem::forget(stream)`. 给 `start_streaming` 补 rustdoc 说明 `StreamPlayerHandle` 必须持有到 `is_done()` 才 Drop
- [x] 1.3 改 `TtsPlayer::play`: cpal callback 闭包里累计已播放样本到 `Arc<AtomicUsize>`, 主线程用 `while played.load(Relaxed) < total { sleep(10ms) }` 等待. 删掉 `thread::sleep(duration_ms + 100)`, 删掉无用的 `is_playing: AtomicBool` 字段
- [x] 1.4 删 `TtsHandler` 上的 `unsafe impl Send` + `unsafe impl Sync` (tts.rs line 30-31). 验证 `cargo check` 仍然通过
- [x] 1.5 给 `TtsHandler`, `TtsPlayer`, `TtsPlayer::new`, `TtsPlayer::play`, `TtsPlayer::start_streaming`, `StreamPlayerHandle` 公共 API 补 `///` rustdoc, 含一句话职责 + 边界 (谁负责 Drop) + `# Examples` 用 ` ```rust,ignore ` (避免 doctest 拉起 cpal/sherpa-onnx)
- [x] 1.6 跑三件套: `cargo fmt --all && cargo clippy --all-features --all-targets -- -D warnings && cargo check --all-features --all-targets`. 全 0 错才进下一步

## 2. 设计文档与 spec 对齐

- [x] 2.1 加强 `crates/ele_bot_server/src/state.rs::SharedState::rebuild_voice` 的 rustdoc: 解释 60ms sleep 的来源 (50ms recv_timeout + 10ms 余量), 为什么 `take()` + `sleep` + `init_voice` + `drop(old)`, 为什么不在 rebuild_voice 里加 cancel 机制 (ASR 走 running flag, TTS 是阻塞 spawn_blocking)
- [x] 2.2 新建 `docs/voice-hot-swap.md`, 内容大纲 (见 design.md 决策 6):
  - 为什么 `rebuild_voice` 不用 `tokio::spawn` 或 `JoinHandle`
  - 为什么 60ms sleep
  - 为什么 drop old Arc 而不是 abort 旧 ASR 线程
  - 为什么 TTS 路径不在 hot-swap cancel 范围
  - 为什么 TTS streaming 的 cpal Stream 必须 RAII (引用本 change 的 1.1/1.2)
- [x] 2.3 在 tts.rs 顶部模块头注释 (`//!`) 里加一行说明本模块与 `state.rs::rebuild_voice` 的交互: cpal Stream 的所有权是 TtsPlayer 调用方的责任, 切音频设备时通过 Arc::drop 链释放

## 3. 提交与最终验证

- [x] 3.1 跑完整三件套最终验证 (确保改动后 0 错 0 warning)
- [x] 3.2 commit. 提交信息遵循 `CLAUDE.md` 规范, 中文 `[<类别>/]` 前缀, 类别建议 `修复` (因为核心是修 hot-swap 路径的隐患). 描述"为什么"而非"做了什么"; 若引用了排查过程 (例如 cargo 缓存脏导致之前跑不通), 在主信息后空一行附上原始现象
- [ ] 3.3 跟用户确认 dev.ps1 工作状态. 当前 `scripts/dev.ps1` 工作区是 CRLF (LF-only 文件在 PS 5.1 parser 下数错行号). 验证 dev.ps1 能跑通 + 切设备的 transient overlay 在客户端能弹 (手动验证或留作用户确认)