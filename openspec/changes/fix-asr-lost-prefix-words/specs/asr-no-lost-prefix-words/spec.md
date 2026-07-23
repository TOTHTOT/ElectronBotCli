## ADDED Requirements

### Requirement: ASR 端到端识别 wav 全部预期文本

端到端跑 `recognition_loop` 识别 `assets/audio/asr_example_zh.wav` 必须命中预期文本"欢迎大家来体验达摩院推出的语音识别模型" (19 字) 的至少 18 字. 共同前缀长度作为"命中度"指标.

**理由**: SenseVoice 模型在长尾分布上可能存在单字符识别错误 (例如把"院"识别为"博"), 此为模型行为而非产品代码问题, 留 1 字错字余地. 但首部 5 字以上丢失必须是产品代码 bug, 不应放过.

#### Scenario: 跑识别测试命中 ≥ 18 字
- **WHEN** 运行 `cargo test --package ele_bot_server test_recognition_no_lost_chars -- --ignored --nocapture`
- **THEN** 测试通过 (`test result: ok`), 共同前缀长度 ≥ 18 字

#### Scenario: 命中 < 18 字时测试 fail
- **WHEN** `recognition_loop` 因产品代码 bug 仍只识别出 ≤ 17 字
- **THEN** `assert!` 触发, 测试 fail 并打印"识别仅 N 字命中, 期望 ≥18. 完整识别: ..."

### Requirement: pre_roll 容量覆盖 VAD 触发延迟 + 前导静音

`PRE_ROLL_MS` 常量必须 ≥ 1500ms, 保证 speech_start 时 `buffer.extend(&pre_roll)` 装入的音频覆盖 wav 真实语音起点前 1.5s 以上的前导段.

**理由**: 实测 wav 真实语音起点 940ms, VAD 触发 1312ms, pre_roll 500ms 只覆盖触发点前 500ms, 漏掉 wav 前 940ms 静音 + VAD 滞后段. 静音虽不影响 SenseVoice 识别 (对照实验 #2 截掉 940ms 静音仍识别 19 字), 但需要足够上下文让解码器归零.

#### Scenario: PRE_ROLL_MS ≥ 1500
- **WHEN** 读 `crates/ele_bot_server/src/media/voice/asr.rs` 第 18 行
- **THEN** `const PRE_ROLL_MS: usize` 的值 ≥ 1500

### Requirement: pre_roll 滑动窗口不超容量上限

`recognition_loop` 的 pre_roll 滑动窗口逻辑必须保证每次 `extend(samples)` 后 `pre_roll.len() ≤ PRE_ROLL_SAMPLES`. 用 `while pre_roll.len() + samples.len() > PRE_ROLL_SAMPLES { pop }` 而非 `while pre_roll.len() >= PRE_ROLL_SAMPLES { pop }`.

**理由**: 旧的 `while >=` 逻辑在 samples=1600 时让 pre_roll 涨到 9599 (> 8000 容量上限 1599 ≈ 100ms). 修复后实测 pre_roll 始终 ≤ 8000.

#### Scenario: 滑动后不超容量
- **WHEN** 任意 chunk samples 进来后
- **THEN** `pre_roll.len()` ≤ `PRE_ROLL_SAMPLES`

### Requirement: VAD accept_waveform 喂真实 chunk 而非 pre_roll 子集

`recognition_loop` 里 `vad.accept_waveform(...)` 必须喂当轮 `samples` (新收到的音频) 的前 512 样本, 而不是 `pre_roll[..512]` 的滑动子集.

**理由**: `accept_waveform` 是"追加新数据"语义, 喂滑动窗口的子集会让 VAD 内部状态每轮重置 32ms 上下文, 跟 cpal 推送的 100ms 帧错位. 这条假设待 VAD 行为进一步验证, 但属于"明显正确"的修复方向.

#### Scenario: VAD 喂入的是当轮 samples
- **WHEN** 读 `crates/ele_bot_server/src/media/voice/asr.rs` 中 `vad.accept_waveform` 调用
- **THEN** 传入切片来自 `samples[..]`, 不是 `all_samples[..]`