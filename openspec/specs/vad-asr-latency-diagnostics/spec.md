# vad-asr-latency-diagnostics Specification

## Purpose
TBD - created by archiving change measure-vad-asr-trigger-latency. Update Purpose after archive.
## Requirements
### Requirement: 测量识别结果的首字丢失量

`asr.rs` 的 `mod tests` SHALL 提供 `test_recognition_no_lost_chars` 测试, 该测试 SHALL 用 `assets/audio/asr_example_zh.wav` 作为输入跑产品 `recognition_loop`, 并 SHALL 打印 (不硬断言) 以下四项:

- 识别结果全文
- 预期文本全文 ("欢迎大家来体验达摩院推出的语音识别模型")
- 二者按字 (`char`) 计算的最长公共前缀长度
- 丢失尾字数量 = 预期字总数 - 公共前缀长度

测试 SHALL 复用现有 `test_recognition_with_audio_file` 的 wav 加载与模型加载方式, 标 `#[ignore]` 与现有约定一致.

#### Scenario: 测试成功运行

- **WHEN** 模型文件已下载, 开发者执行 `cargo test -p ele_bot_server -- --ignored --nocapture test_recognition_no_lost`
- **THEN** 控制台打印识别结果 / 预期 / 共同前缀 / 丢失字数四项, 测试 pass (因为不硬断言)

#### Scenario: 缺失模型文件

- **WHEN** `ModelManager::global().get("sense_voice")` 返回 `None`
- **THEN** 测试因 `expect` panic 报错, 提示模型未找到 (与现有 `test_recognition_with_audio_file` 行为一致)

### Requirement: 测量 VAD 触发延迟

`asr.rs` 的 `mod tests` SHALL 提供 `test_vad_trigger_latency` 测试, 该测试 SHALL:

- 加载 `asr_example_zh.wav` 转为 f32
- 用 16ms 滑动窗口 RMS 找到"真实语音起点"样本序号 (`first_speech_sample`)
- 按 cpal 帧大小 512 样本逐帧 `vad.accept_waveform`, 记录 `vad.detected()` 首次返回 `true` 的样本序号
- 打印真实语音起点 (ms) / VAD 触发点 (ms) / 触发延迟 (ms) / 约等于多少字 (按 250ms/字估)
- **不**硬断言, 仅打印

测试 SHALL 标 `#[ignore]`.

#### Scenario: 测试成功运行

- **WHEN** 模型已下载, 执行 `cargo test -p ele_bot_server -- --ignored --nocapture test_vad_trigger_latency`
- **THEN** 控制台打印真实语音起点 ms / VAD 触发点 ms / 触发延迟 ms / 约 N 字, 测试 pass

#### Scenario: 数字反映真实行为

- **WHEN** 测试打印的触发延迟 > 250ms
- **THEN** 报告数字, 供开发者判断是否需要加 pre_roll

### Requirement: 测量 pre_roll 实际捕获的前文时长

`asr.rs` 的 `mod tests` SHALL 提供 `test_pre_roll_capture_rate` 测试, 该测试 SHALL:

- 用与 `test_vad_trigger_latency` 相同的方法找到真实语音起点 N_real
- 模拟 `recognition_loop` 的 pre_roll 行为 (VecDeque 容量 8000 样本 = 500ms), 逐帧 push + 弹出最旧
- 在 VAD 触发那一刻 (`n_vad`), 检查 pre_roll 中 N_real 之前的样本数 `n_pre`
- 打印 pre_roll 实际捕获前文时长 (ms) / 理论值 (ms = 500) / 差值
- **不**硬断言, 仅打印

测试 SHALL 标 `#[ignore]`.

#### Scenario: 测试成功运行

- **WHEN** 模型已下载, 执行 `cargo test -p ele_bot_server -- --ignored --nocapture test_pre_roll_capture_rate`
- **THEN** 控制台打印捕获前文 ms / 理论 ms / 差值 ms, 测试 pass

#### Scenario: 数字反映真实行为

- **WHEN** 实际捕获前文 < 500ms
- **THEN** 报告数字, 供开发者判断 pre_roll 是否被提前截断

### Requirement: 测试代码不修改产品逻辑

上述三个测试 SHALL 仅向 `mod tests` 添加新函数与辅助函数 (`first_speech_sample`, `longest_common_prefix_chars`, `simulate_pre_roll`), 不得修改 `recognition_loop` / `peak_to_volume` / `process_audio_chunk` / `build_asr_stream` / `recognition_thread` 任一产品函数.

#### Scenario: 跑现有 cargo fmt / clippy / check

- **WHEN** 三个测试加完后, 执行 `cargo fmt --all && cargo clippy --all-features --all-targets -- -D warnings && cargo check --all-features --all-targets`
- **THEN** 三项全部通过, 且不报 dead_code / unused_imports (因为辅助函数会被三个测试使用)

