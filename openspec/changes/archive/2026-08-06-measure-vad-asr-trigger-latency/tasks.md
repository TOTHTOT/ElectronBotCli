## 1. 辅助函数

- [x] 1.1 在 `asr.rs` 的 `mod tests` 里新增 `first_speech_sample(samples: &[f32], threshold_dbfs: f32) -> usize`, 带 rustdoc 说明"16ms 滑动窗口 RMS 首次超阈值的样本序号", 私有 fn
- [x] 1.2 在 `mod tests` 里新增 `longest_common_prefix_chars(a: &str, b: &str) -> usize`, 按 `char` 数公共前缀, 带 rustdoc
- [x] 1.3 在 `mod tests` 里新增 `simulate_pre_roll(samples: &[f32], vad_trigger_sample: usize) -> usize`, 返回 pre_roll 中真实语音起点之前的样本数, 带 rustdoc 说明"仅用于测量, 不保证与 recognition_loop 行为完全一致"

## 2. 三个测量测试

- [x] 2.1 新增 `test_recognition_no_lost_chars`, 跑 `recognition_loop` 拿结果, 打印识别/期望/共同前缀/丢失字数四项, 不硬断言, 标 `#[ignore]`
- [x] 2.2 新增 `test_vad_trigger_latency`, 找真实语音起点 + 逐帧喂 VAD, 打印延迟 (ms) 与约多少字, 不硬断言, 标 `#[ignore]`
- [x] 2.3 新增 `test_pre_roll_capture_rate`, 模拟 pre_roll, 在 VAD 触发那一刻量实际前文捕获量, 打印捕获 ms / 理论 ms / 差值, 不硬断言, 标 `#[ignore]`

## 3. 验证

- [x] 3.1 完成后跑三件套: `cargo fmt --all && cargo clippy --all-features --all-targets -- -D warnings && cargo check --all-features --all-targets` 三项必须全过
- [x] 3.2 跑三个 ignored 测试 (模型已下载的前提下): `cargo test -p ele_bot_server -- --ignored --nocapture test_recognition_no_lost test_vad_trigger_latency test_pre_roll_capture_rate`, 记录三组打印数字, 用于后续修复决策