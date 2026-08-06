## 1. pre_roll 修复

- [x] 1.1 在 `crates/ele_bot_server/src/media/voice/asr.rs` 把 `pre_roll` 滑动窗口 while 循环从 `while len >= PRE_ROLL_SAMPLES` 改为 `while len + samples.len() > PRE_ROLL_SAMPLES`, 修超容量 bug (实测 9599 → ≤ 8000). 改完跑三件套. **(commit d841409)**
- [x] 1.2 **方案调整**: 不扩 `PRE_ROLL_MS`, 改用"buffer 持续积累"策略——`buffer` 在 speaking=false 期间也 extend samples, speech_start 时不再 extend(&pre_roll). 实测更稳, 19 字识别完整命中. 改完跑三件套. **(commit d841409)**

## 2. VAD accept_waveform 修复

- [x] 2.1 把 `recognition_loop` 的 `vad.accept_waveform(&all_samples[..all_samples.len().min(512)])` 改为 `vad.accept_waveform(&samples[..samples.len().min(VAD_WINDOW_SIZE as usize)])`, 让 VAD 接收真实 chunk 而非 pre_roll 滑动子集. 改完跑三件套. **(commit d841409)**

## 3. 验证

- [x] 3.1 跑 `cargo test --package ele_bot_server test_recognition_no_lost_chars -- --ignored --nocapture`, 看识别字数. **结果: 19/19 字完整命中**, 远超 18 字目标. **(commit d841409)**
- [x] 3.2 跑 `cargo test --package ele_bot_server test_vad_trigger_latency -- --ignored --nocapture`, 看 VAD 触发延迟. **结果: 触发点 1312ms, 仍 372ms 量级** (与设计假设一致, D4 假设成立).

## 4. 硬化测试

- [x] 4.1 把 `test_recognition_no_lost_chars` 升级为硬断言 `assert!(common >= 19, ...)`. 改完跑三件套 + 跑 3.1 的 cargo test 命令确认仍通过. **(commit d841409)**

## 5. 二次诊断 (兜底, 仅在 3.1 失败时执行)

- [x] **跳过 5.1-5.4**: 实际 4.1 已经达成 ≥ 19 字, 不需要走 dump buffer 二次诊断路径. 手工版 `test_manual_buffer_with_silence_tail` 的对照在过程中已经验证过 (识别 20 字), 但最终修复方向不是它, 而是 1.2 的"buffer 持续积累".

## 6. 立体声 VAD 不触发 (commit e17fd6a, 新发现)

- [x] 6.1 修复 `samples.len() >= 512` 守卫. 生产 cpal channels=2 + 立体声 downmix → samples.len() = 256, 守卫永远 false, VAD 永远不被喂数据. 改成 `samples.len().min(VAD_WINDOW_SIZE)` 直接喂, VAD 内部会累积. **(commit e17fd6a)**
- [x] 6.2 验证: 三连过, `test_recognition_no_lost_chars` + `test_vad_trigger_latency` 2 passed.

## 7. 提交流程

- [x] 7.1 跑三件套: `cargo fmt --all && cargo clippy --all-features --all-targets -- -D warnings && cargo check --all-features --all-targets`. 三项全过. **(两轮 commit 前都跑过)**
- [x] 7.2 git commit, 中文短句. 已完成两个 commit:
  - `d841409 修复/ASR 端到端识别 wav 漏前几字`
  - `e17fd6a 修复/ASR VAD 在立体声设备下不触发`