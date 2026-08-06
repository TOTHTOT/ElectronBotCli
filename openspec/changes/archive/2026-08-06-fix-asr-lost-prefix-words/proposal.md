## Why

`crates/ele_bot_server/src/media/voice/asr.rs` 的 `recognition_loop` 在 VAD 触发时通过 `pre_roll` 缓冲"触发点前 500ms"的音频喂给 SenseVoice, 但实测 `assets/audio/asr_example_zh.wav` (预期文本"欢迎大家来体验达摩院推出的语音识别模型", 19 字) 只能识别出 14 字, 丢了前 5 字. 用户反馈首字丢失明显, 必须修复.

## What Changes

- 修 `recognition_loop` 的 `pre_roll` 滑动窗口 bug (`while len >= target` 让容量超限 1599 样本)
- 把 `PRE_ROLL_MS` 从 500 调到能覆盖 silero_vad 内部缓冲 + VAD 触发延迟的值
- 在 `recognition_loop` 的 buffer 拼接处加入"前导静音"识别逻辑, 让 SenseVoice 看到完整时间线
- 把 `test_recognition_no_lost_chars` 升级为硬断言: 共同前缀必须 ≥ 18 字 (允许最多 1 字错字, 因为 SenseVoice "院→博" 单字符错误是模型行为而非产品代码问题)
- 保留 `measure-vad-asr-trigger-latency` change 的诊断测试作为回归参考

## Capabilities

### New Capabilities

- `asr-no-lost-prefix-words`: 端到端识别 `asr_example_zh.wav` 必须命中 19 字预期文本的至少 18 字. 落硬断言, 回归即 fail.

### Modified Capabilities

无.

## Impact

- 改 crate: `crates/ele_bot_server` (仅 `src/media/voice/asr.rs`)
- 不改 `crates/ele_bot_client`, `crates/ele_bot_proto`
- 不引入新依赖 (复用现有 `sherpa_onnx::OfflineRecognizer` / `VoiceActivityDetector`)
- 运行时无影响 (仅修改 ASR 模块的拼接逻辑, 协议层不变)

## Non-goals

- 不改 SenseVoice 模型本身, 不改模型推理参数 (temperature / beam_size)
- 不改 silero_vad 的 `threshold` / `min_silence_duration` / `min_speech_duration` (本次修复不动 VAD 配置)
- 不修"院→博"单字符错字 (那是 SenseVoice 模型行为, 调产品代码无效)
- 不动 `MIN_AUDIO_LEN` / `SILENCE_THRESHOLD` 等其他 ASR 常量 (除非诊断明确指向它们)