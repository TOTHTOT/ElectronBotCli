## 1. pre_roll 修复

- [ ] 1.1 在 `crates/ele_bot_server/src/media/voice/asr.rs` 把 `pre_roll` 滑动窗口 while 循环从 `while len >= PRE_ROLL_SAMPLES` 改为 `while len + samples.len() > PRE_ROLL_SAMPLES`, 修超容量 bug (实测 9599 → ≤ 8000). 改完跑三件套.
- [ ] 1.2 把 `PRE_ROLL_MS` 从 500 提到 2000 (覆盖 silero_vad 600ms 内部缓冲 + VAD 触发滞后 372ms + 1s 前导). 改完跑三件套.

## 2. VAD accept_waveform 修复

- [ ] 2.1 把 `recognition_loop` 第 ~114 行 `vad.accept_waveform(&all_samples[..all_samples.len().min(512)])` 改为 `vad.accept_waveform(&samples[..samples.len().min(512)])`, 让 VAD 接收真实 chunk 而非 pre_roll 滑动子集. 改完跑三件套.

## 3. 验证

- [ ] 3.1 跑 `cargo test --package ele_bot_server test_recognition_no_lost_chars -- --ignored --nocapture`, 看识别字数. 若 ≥ 18 字: 进入第 4 节. 若 < 18 字: 进入第 5 节 (二次诊断).
- [ ] 3.2 跑 `cargo test --package ele_bot_server test_vad_trigger_latency -- --ignored --nocapture`, 看 VAD 触发延迟是否仍在 372ms 量级 (±100ms 可接受). 若差异 > 200ms 说明 D4 假设错了, 回滚 2.1.

## 4. 硬化测试

- [ ] 4.1 把 `test_recognition_no_lost_chars` 从"只打印不断言"升级为硬断言: `assert!(common >= 18, ...)`. 改完跑三件套 + 跑 3.1 的 cargo test 命令确认仍通过.

## 5. 二次诊断 (兜底, 仅在 3.1 失败时执行)

- [ ] 5.1 在 `recognition_loop` 的 `if buffer.len() > MIN_AUDIO_LEN` 分支前, 把 `buffer.clone()` 写到一个 `Arc<Mutex<Vec<f32>>>` (参数化进 `recognition_loop`). 改完跑三件套.
- [ ] 5.2 在 `test_recognition_no_lost_chars` 里 clone 出真实 buffer 后, **再用同一个 recognizer 新建 stream 喂这份 buffer 看真实识别结果**. 跟手工版 `test_manual_buffer_with_silence_tail` 对照. 找到差异点.
- [ ] 5.3 根据 5.2 发现的差异, 在 `recognition_loop` 里做针对性修复 (例如改 VAD 触发判定 / 改 buffer 拼接顺序 / 改 sense_voice decoder 调用方式). 改完跑 3.1 验证.
- [ ] 5.4 若 5.3 仍未达 ≥ 18 字, 把 `silero_vad` 配置里 `min_silence_duration: 0.3` 调到 `0.15` + `min_speech_duration: 0.3` 调到 `0.15`, 让 VAD 触发更激进. 改完跑 3.1 + 3.2 验证 VAD 滞后是否减小.

## 6. 提交流程

- [ ] 6.1 跑三件套: `cargo fmt --all && cargo clippy --all-features --all-targets -- -D warnings && cargo check --all-features --all-targets`. 三项全过才能提交.
- [ ] 6.2 git commit, 中文短句, 首行格式 `修复/ASR 端到端识别 wav 漏前几字`.