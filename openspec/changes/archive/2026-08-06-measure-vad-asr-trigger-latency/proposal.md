## Why

仓库用户反馈: 接 VAD 后做 SenseVoice 识别, 实际识别结果会丢失前面几个字 (例如 `assets/audio/asr_example_zh.wav` 的预期文本"欢迎大家来体验达摩院推出的语音识别模型", 在端到端跑通后首字可能丢失).

仓库目前只有 `test_recognition_with_audio_file` (asr.rs:295), 它只断言"识别有结果", 没有量具体丢了几个字 / VAD 实际触发延迟多少毫秒 / pre_roll 实际捕获多少前文. 没有这三组数字, 修复时无法判断根因到底是 VAD 延迟过长、pre_roll 没装满, 还是 buffer 拼接/截断逻辑把首字砍掉了.

## What Changes

- 在 `crates/ele_bot_server/src/media/voice/asr.rs` 的 `mod tests` 里**新增三个 `#[test]`** (不改任何产品代码):
  - `test_recognition_no_lost_chars` — 用 `asr_example_zh.wav` 跑 `recognition_loop`, 计算结果与预期文本的最长公共前缀, 打印"识别/期望/共同前缀/丢失字数".
  - `test_vad_trigger_latency` — 找出 wav 中真实语音起点 (滑动窗口 RMS 首超阈值的样本), 逐 512 样本帧喂 VAD, 打印"VAD 触发延迟 (ms)" 与"约等于多少字".
  - `test_pre_roll_capture_rate` — 模拟 `recognition_loop` 的 pre_roll 行为, 在 VAD 触发那一刻检查 pre_roll 里实际有多少毫秒的"真实语音前文"被保留.
- 这三个测试**只**用来打印诊断数字, **不**做硬断言. 跑一次后用数字对话, 再决定下一步修法.

## Non-goals

- 不动 `recognition_loop` / `peak_to_volume` / `build_asr_stream` 等任何产品代码
- 不改 VAD / SenseVoice 配置参数 (`min_speech_duration`, `threshold`, `PRE_ROLL_MS` 等)
- 不引入新依赖 (复用 `sherpa_onnx::VoiceActivityDetector` 和现有 `ModelManager`)
- 不把这些测试接入 CI (`#[ignore]` 标志位保留, 模型存在才跑)

## Capabilities

### New Capabilities

- `vad-asr-latency-diagnostics`: 在 ASR 模块下提供一组仅用于打印的延迟/丢字测量测试, 把"丢几个字 / VAD 触发延迟 / pre_roll 实际捕获时长"三个数值暴露出来, 供后续修复方向决策.

### Modified Capabilities

无.

## Impact

- 改 crate: `crates/ele_bot_server` (仅 `src/media/voice/asr.rs` 的 `mod tests`)
- 依赖: 复用现有 `sherpa_onnx`, `cpal`, `ModelManager`
- 不影响 `crates/ele_bot_client`, `crates/ele_bot_proto`, 不改 `Cargo.toml`
- 运行时无影响 (测试代码仅在 `cargo test` 时编译运行)