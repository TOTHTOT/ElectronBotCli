## ADDED Requirements

### Requirement: ASR 端到端识别 wav 全部预期文本

端到端跑 `recognition_loop` 识别 `assets/audio/asr_example_zh.wav` MUST 命中预期文本"欢迎大家来体验达摩院推出的语音识别模型" (19 字) 的至少 18 字. 共同前缀长度作为"命中度"指标.

**理由**: SenseVoice 模型在长尾分布上可能存在单字符识别错误, 此为模型行为而非产品代码问题, 留 1 字错字余地. 但首部 5 字以上丢失 MUST 是产品代码 bug, 不应放过.

#### Scenario: 跑识别测试命中 ≥ 18 字
- **WHEN** 运行 `cargo test --package ele_bot_server test_recognition_no_lost_chars -- --ignored --nocapture`
- **THEN** 测试通过 (`test result: ok`), 共同前缀长度 ≥ 18 字

#### Scenario: 命中 < 18 字时测试 fail
- **WHEN** `recognition_loop` 因产品代码 bug 仍只识别出 ≤ 17 字
- **THEN** `assert!` 触发, 测试 fail 并打印"识别仅 N 字命中, 期望 ≥18. 完整识别: ..."

### Requirement: buffer 持续积累保证 VAD 触发时包含 wav 起点

`recognition_loop` MUST 在 `speaking=false` 期间也持续 `buffer.extend(&samples)`. `speech_start` 时 MUST **不**调用 `buffer.extend(&pre_roll)`——`buffer` 此时已包含 wav 起点静音段, SenseVoice 看到完整时间线.

**理由**: 旧逻辑"speaking=false 时 buffer 不增长, speech_start 时 extend(pre_roll)"让 buffer 跳过了 wav 起点, SenseVoice 识别丢前 5 字. buffer 持续积累 + 不再 extend(pre_roll) 后, 实测 19/19 字完整命中.

#### Scenario: buffer 在静音段也累积
- **WHEN** 读 `crates/ele_bot_server/src/media/voice/asr.rs` 中 `recognition_loop`
- **THEN** 存在 `else { buffer.extend(&samples); }` 分支 (is_speech=false 且 speaking=false 时)

#### Scenario: speech_start 时不再 extend pre_roll
- **WHEN** 读 `crates/ele_bot_server/src/media/voice/asr.rs` 中 `if !speaking { ... }` 分支
- **THEN** 不包含 `buffer.extend(&pre_roll)`

### Requirement: pre_roll 滑动窗口不超容量上限

`recognition_loop` 的 pre_roll 滑动窗口逻辑 MUST 保证每次 `extend(samples)` 后 `pre_roll.len() ≤ PRE_ROLL_SAMPLES`. 实现 MUST 用 `while pre_roll.len() + samples.len() > PRE_ROLL_SAMPLES { pop_front }` 而非 `while pre_roll.len() >= PRE_ROLL_SAMPLES { pop_front }`.

**理由**: 旧的 `while >=` 逻辑在 samples=1600 时让 pre_roll 涨到 9599 (> 8000 容量上限 1599 ≈ 100ms). 修复后实测 pre_roll 始终 ≤ 8000.

#### Scenario: 滑动后不超容量
- **WHEN** 任意 chunk samples 进来后
- **THEN** `pre_roll.len()` ≤ `PRE_ROLL_SAMPLES`

### Requirement: VAD accept_waveform 喂当轮真实 chunk

`recognition_loop` 的 `vad.accept_waveform(...)` MUST 喂当轮 `samples` (新收到的音频) 的前 N 样本 (N = samples.len().min(VAD_WINDOW_SIZE)), 而 **不**是 `pre_roll[..512]` 的滑动子集. MUST 不带 `samples.len() >= 512` 守卫, 否则立体声设备下 VAD 永远不被喂数据.

**理由**:
1. `accept_waveform` 是"追加新数据"语义, 喂滑动窗口的子集会让 VAD 内部状态错位.
2. 生产 cpal 配置 channels=2, `process_audio_chunk` 里立体声 downmix 到单声道, cpal Fixed(512) 立体声帧 → `samples.len() = 256`. 若有 `samples.len() >= 512` 守卫则永远 false, VAD 永远不被喂数据, `vad.detected()` 永远 false.

#### Scenario: VAD 喂入当轮 samples 无长度守卫
- **WHEN** 读 `crates/ele_bot_server/src/media/voice/asr.rs` 中 `vad.accept_waveform` 调用
- **THEN** 传入切片来自 `samples[..feed_n]` (feed_n = samples.len().min(VAD_WINDOW_SIZE)), 没有 `samples.len() >= 512` 这种会跳过 VAD 喂入的守卫