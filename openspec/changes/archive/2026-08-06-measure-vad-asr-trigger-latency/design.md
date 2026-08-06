## Context

- `crates/ele_bot_server/src/media/voice/asr.rs` 当前有 3 个产品函数 (`recognition_loop`, `peak_to_volume`, `build_asr_stream`) + 3 个 `#[cfg(test)]` 测试 (`test_load_wav_samples`, `peak_to_volume_mapping`, `test_recognition_with_audio_file`).
- `test_recognition_with_audio_file` 已实现 wav 喂识别 (`#[ignore]`), 但只断言 `!results.is_empty()`, 不报告丢了几个字.
- 现有 wav `assets/audio/asr_example_zh.wav` 是 16kHz mono 16-bit PCM (已 `file` 确认), 内容"欢迎大家来体验达摩院推出的语音识别模型", 时长 ≈ 5.5s, 是天然的 ground truth.
- VAD 用 sherpa-onnx 的 `VoiceActivityDetector` (Silero VAD), 在 `init_silero_vad` 里创建, 配置 `min_speech_duration: 0.3` / `min_silence_duration: 0.3` / `threshold: 0.5`.
- 产品 `recognition_loop` 的循环节奏: cpal 帧大小 512 样本 (32ms) → audio_rx `recv_timeout(50ms)` → VAD 用 `pre_roll[..512]` 喂 → `detected()` 判定.

## Goals / Non-Goals

**Goals:**

- 三个测试只**打印数字**, 不做硬断言, 不破坏现有测试
- 跑一次得到: (1) 实际丢几个字; (2) VAD 触发延迟 ms; (3) pre_roll 实际装到的前文 ms
- 测试只在模型文件存在时才跑 (`#[ignore]`, 与现有 `test_recognition_with_audio_file` 一致)
- 测试代码遵守 CLAUDE.md: 公共 API 必须 rustdoc, 行内注释中文

**Non-Goals:**

- 不动产品代码
- 不修 VAD 参数 / pre_roll 长度 / SenseVoice 配置
- 不接入 CI
- 不写自动化断言 (修复阶段再加)

## Decisions

### Decision 1: 三个测试放在 `asr.rs` 的 `mod tests` 里, 不单开文件

- **理由**: 与现有 `test_recognition_with_audio_file` 共用 `ModelManager` / wav 加载工具, 避免跨文件依赖. 修改面最小.
- **备选**: 新建 `tests/latency.rs` 集成测试 — 拒绝, 因为要重新导出私有 fn, 改动大.

### Decision 2: 真实语音起点用"滑动窗口 RMS 首超阈值"

```rust
/// 找到样本中能量首次超过 dBFS 阈值的样本序号.
///
/// 滑动窗口 16ms (256 样本), 计算窗口内 RMS, 第一个 ≥ 阈值的窗口起点
/// 即为"真实语音起点". 用于对照 VAD.detected() 的样本位置, 算出触发
/// 延迟. 仅做测量用, 不参与产品逻辑.
fn first_speech_sample(samples: &[f32], threshold_dbfs: f32) -> usize
```

- **理由**: RMS (而不是 peak) 对真实人声更稳定; 阈值 -30 dBFS 是常用口语起点判别.
- **备选**: 短时能量 / ZCR / 过零率 — 拒绝, RMS 已够用, 多算反而引入新参数.

### Decision 3: VAD 喂法模拟真实循环节奏 (按 512 样本切帧)

```rust
for chunk in samples.chunks(512) {
    vad.accept_waveform(chunk);
    if vad.detected() && !recorded {
        n_vad = i * 512;
        recorded = true;
        break;
    }
}
```

- **理由**: 与产品 `recognition_loop` 一致, 量出来的延迟才能反映真实行为. 如果一次性把整段喂进去, VAD 状态机会"作弊"提前触发.
- **备选**: 一次性喂整段 — 拒绝, 数字会偏乐观.

### Decision 4: pre_roll 捕获测试用**仿真**, 不复用 `recognition_loop`

- **理由**: `recognition_loop` 的 pre_roll 是局部变量 (`VecDeque`), 外部看不到, 也不容易注入. 自己写一个 30 行的 `simulate_pre_roll(&samples) -> (VecDeque, usize /* 触发点 */)` 仿真更可控.
- **备选**: 暴露 `pre_roll` 给测试 — 拒绝, 是产品内部细节, 不应该被测试代码耦合.

### Decision 5: 文本比对用 `longest_common_prefix`, 不强求完全相等

```rust
/// 计算两个字符串按 char (而不是 byte) 切片的最长公共前缀长度.
/// 用于报告 ASR 输出与 ground truth 的对齐度.
fn longest_common_prefix_chars(a: &str, b: &str) -> usize
```

- **理由**: 中文按 UTF-8 是变长字节, `str::chars().zip(...)` 才能正确数"几个字".
- **备选**: 字节级 prefix — 拒绝, 切到字中间会 panic.

### Decision 6: 测试全部标 `#[ignore]`, 与现有 `test_recognition_with_audio_file` 一致

- **理由**: 依赖模型文件 (`ModelManager::global().get(...)`), CI 默认 `cargo test` 不会拉模型.
- **手动跑法**: `cargo test -p ele_bot_server -- --ignored --nocapture test_vad_ test_pre_roll test_recognition_no_lost`

## Risks / Trade-offs

- [Risk] **数字只能反映该 wav 的特征** — `asr_example_zh.wav` 是单条样本, 量出来的延迟不能外推到所有场景
  → Mitigation: 在打印结果里附上 wav 路径和时长, 提醒这是单条样本的测量
- [Risk] **VAD 模型在 CPU 上加载慢, 测试首次跑会卡住**
  → Mitigation: `#[ignore]` 默认跳过, 主动 `--ignored` 才跑; 在打印里说明"首次跑需等模型加载"
- [Risk] **longest_common_prefix 可能因为标点/语气词产生 1 字偏差, 误判"丢字"**
  → Mitigation: 同时打印"识别结果全文"和"丢失尾字", 人眼复核
- [Risk] **pre_roll 仿真和真实 `recognition_loop` 行为有微妙差异, 数字可能误导**
  → Mitigation: 仿真函数单独标 `// 仅用于测量, 不保证与 recognition_loop 行为完全一致`, 输出与真实循环对比时明确标注

## Open Questions

- 修复阶段是否需要把这些数字变成硬断言? (本次**不**做, 等数字出来再决定)
- 量完之后, 修法选 A (加 pre_roll) / B (改 VAD 喂法) / A+B 哪个? — 等数字出来再讨论.