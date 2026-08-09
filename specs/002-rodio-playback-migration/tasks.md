# Tasks: rodio 替换手写 cpal 播放层

**Input**: plan.md / research.md / data-model.md / quickstart.md @ specs/002-rodio-playback-migration/

## Phase 1: Setup

- [X] T001 确认 rodio 依赖就位: `crates/ele_bot_server/Cargo.toml` 含
  `rodio = { version = "0.22.2", default-features = false, features = ["playback"] }`,
  `cargo check -p ele_bot_server` 依赖解析通过
- [X] T002 grep `OwnedOutputStream` 全部使用方, 确认仅 `tts.rs` 播放层使用
  (研究遗留风险项)

## Phase 2: Core (单文件重写, 顺序执行)

- [X] T003 重写 `crates/ele_bot_server/src/media/voice/tts.rs` 播放层:
  `TtsPlayer` 改持 `MixerDeviceSink` (`DeviceSinkBuilder::from_device`
  + `open_sink_or_fallback`); `play()` 改 `Player::append(SamplesBuffer)`
  + `sleep_until_end()`; `start_streaming()` 改 `queue::queue(true)`
  + `write_chunk` → `tx.append(SamplesBuffer)`, `mark_synthesis_done`
  翻 keep_alive, `is_done` = `synthesis_done && player.empty()`;
  公开 API 签名不变
- [X] T004 删除死代码: `write_audio_callback`/`_i16`、`detect_sample_format`、
  `build_stream`/`build_stream_with_fallback`、`drain_into`、`F32Callback`/
  `I16Callback`、`sample_format` 字段、`OwnedOutputStream` (依 T002 结果);
  修复 `voice/mod.rs` 及测试的编译引用

## Phase 3: Validation

- [X] T005 质量门禁: `cargo fmt --all && cargo clippy --all-features --all-targets -- -D warnings && cargo test -p ele_bot_server`
- [X] T006 [US1] 部署 lckfb (`deploy_rk3566.sh all`), quickstart 场景 1/2:
  `TtsSpeak streaming:false/true` 实机播放正常, 日志无格式错误
- [X] T007 [US3] 结构验收: 播放层无 SampleFormat/设备特判分支,
  行数较 b8db23d 净减 ≥50%, `OwnedOutputStream` 无残留;
  更新 spec 勾选与 README 待优化项(如适用)

**依赖**: T001→T002→T003→T004→T005→T006→T007 全顺序 (单文件重写, 无并行项)
