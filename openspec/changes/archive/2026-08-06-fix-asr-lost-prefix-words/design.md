## Context

### 现状
- `recognition_loop` (asr.rs:70-156) 跑 VAD 检测 + SenseVoice 离线识别, 把音频按 chunk 从 `audio_rx` 收上来, 用 `pre_roll` 滑动窗口保留最近 N 毫秒, VAD 触发 (`vad.detected() == true` 首次从 false 翻 true) 时把 `pre_roll` 全 extend 进 `buffer`, 之后每轮 `samples` 也 extend 进 `buffer`, silence_count 超阈值触发 `recognizer.decode`.
- `assets/audio/asr_example_zh.wav` 预期文本"欢迎大家来体验达摩院推出的语音识别模型" (19 字), 实测识别 14 字 ("验达摩院推出的语音识别模型。").
- wav 物理特性: 5.5s, 16kHz 单声道 i16 PCM (88747 样本). -30dBFS RMS 阈值下真实语音起点 = 940ms (样本 15045); silero_vad 触发点 = 1312ms (样本 20992). 也就是说 wav 有 900ms 静音头.

### 已做的诊断 (`measure-vad-asr-trigger-latency` change 的测试已跑)
| 数字 | 值 |
|---|---|
| wav 总长 | 88747 样本 / 5.55s |
| 真实语音起点 (-30dBFS) | 15045 样本 / 940ms |
| silero_vad 触发点 | 20992 样本 / 1312ms |
| VAD 触发延迟 | 372ms (≈ 1.5 字 @250ms/字) |
| pre_roll 容量 (旧) | 8000 样本 / 500ms |
| pre_roll 容量 (新) | 32000 样本 / 2000ms |
| pre_roll 实测容量 (旧 while 循环) | 9599 样本 (超出 1599 ≈ 100ms) |

### 已做的对照实验 (`asr.rs` mod tests 新增 5 个, 全部 `#[ignore]`)
| 实验 | 喂的内容 | 识别字数 |
|---|---|---|
| `test_raw_wav_no_vad` | 整段 wav 直喂 SenseVoice | **20 (含句号)** ✓ |
| `test_raw_wav_trim_silence_head` | 截掉 940ms 静音头后整段 | **20** ✓ |
| `test_raw_wav_head_offset` | 留 1500ms 前导 + 完整语音 | **20** ✓ |
| `test_raw_wav_with_long_silence_tail` | wav + 12s 静音尾巴 | **20** ✓ |
| `test_manual_buffer_pre_roll_2000ms` | 手工拼接 buffer (hardcoded VAD 触发, 无 VAD 调用) | **20** ✓ |
| `test_manual_buffer_with_silence_tail` | 手工拼接 + 12s 静音尾巴 | **20** ✓ |
| **真实 `recognition_loop`** | (上述所有修改都试过) | **14** ✗ |

### 矛盾点
- 模型 + wav + 手工 buffer 都能识别 20 字.
- 真实 `recognition_loop` 走的是 `silero_vad.detected()` 真 VAD, 与手工 hardcoded 触发点 (20992) 的差异来源未 100% 定位.
- 候选根因: (a) silero_vad 内部缓冲 600ms 让 `detected()` 真正翻 true 的时点与 20992 不严格等价, (b) pre_roll 在 speaking=false 期间不积累导致 buffer 头缺前导, (c) VAD `accept_waveform` 每轮喂 512 样本但只覆盖 pre_roll 前 512 = 32ms 而非全部 1600 chunk 的内容, 让 VAD 内部"看到的语音段"短于 wav 真实语音段.

## Goals / Non-Goals

**Goals:**
- 让 `test_recognition_no_lost_chars` 在跑真实 `recognition_loop` 时识别 ≥ 18 字 (允许 1 字错字给 SenseVoice 模型行为留余地).
- 不引入新依赖.
- 不改 SenseVoice 模型配置 / silero_vad 配置 (除非定位明确指向它们).
- 不动协议层 (`ClientMessage` / `ServerEvent`).

**Non-Goals:**
- 不修"院→博"单字符错字 (SenseVoice 模型行为, 调产品代码无效).
- 不修 TTS 端丢字.
- 不动 LLM / FaceTracker / 摄像头等其它模块.
- 不动 OpenSpec 已归档 change.

## Decisions

### D1: pre_roll 容量 500ms → 2000ms

**理由**: wav 实测有 900ms 静音头 + VAD 触发延迟 372ms + silero_vad 内部缓冲 ~600ms. 旧 500ms 让 buffer 头只装到 wav [812ms, 1312ms], 漏掉 [0, 812ms] (虽然静音, SenseVoice 可能需要它做"上下文归零"). 新 2000ms 覆盖整个前导段.

**替代方案**:
- 保留 500ms + 让 `buffer` 在 `speaking=false` 期间也积累 `samples` —— 工作量更大, 改动面更广.
- 把 `pre_roll` 装所有 `audio_rx` 收上来的历史 —— 内存不可控.

### D2: 修 `pre_roll` 滑动窗口 while 循环

```rust,ignore
// 旧 (bug): while len >= cap { pop } then extend(samples), 实测涨到 9599
while pre_roll.len() >= PRE_ROLL_SAMPLES {
    pre_roll.pop_front();
}
pre_roll.extend(&samples);

// 新: 一次性 pop 到位, 保证 extend 后容量 ≤ cap
while pre_roll.len() + samples.len() > PRE_ROLL_SAMPLES {
    pre_roll.pop_front();
}
pre_roll.extend(&samples);
```

**理由**: 旧的 `while >= cap` 退出来时 pre_roll 是 cap-1 (7999), 然后 extend(samples) 加 1600 = 9599, **超 cap 1599**. 改用 `while len + samples.len() > cap` 直接 pop 到 ≤ cap - samples.len().

**影响**: 微不足道, 一次性多 pop 几次而已.

### D3: 升级 `test_recognition_no_lost_chars` 为硬断言

```rust,ignore
let common = longest_common_prefix_chars(&recognized, expected);
assert!(
    common >= 18,
    "识别仅 {} 字命中, 期望 ≥18. 完整识别: {:?}",
    common,
    recognized
);
```

**理由**: proposal 要求"识别出完整内容", 软断言 (只打印) 没用. 硬断言 ≥ 18 字给 SenseVoice "院→博" 错字留 1 字余地, 但保证 19 字中至少 18 字命中.

**替代方案**:
- 硬断言 19 字完全相等 —— 太严, 模型单字错会误报回归.
- 硬断言 ≥ 15 字 —— 太松, 没意义.

### D4: VAD `accept_waveform` 调用从"每轮 pre_roll[..512]"改为"每轮 samples[..512]"

```rust,ignore
// 旧 (可能 bug): 每轮喂 pre_roll 前 512 = 32ms, VAD 内部状态被每轮重置窗口
vad.accept_waveform(&all_samples[..all_samples.len().min(512)]);

// 新: 每轮喂刚收上来的 samples (1600 样本 = 100ms), VAD 累积真实 chunk
vad.accept_waveform(&samples[..samples.len().min(512)]);
```

**理由**: `pre_roll` 是滑动窗口, 内容每轮在变; VAD `accept_waveform` 接收新数据应该喂"新到的音频", 不是"窗口的前 32ms". 当前实现让 VAD 看到的总是 pre_roll 最老的 32ms, 跟实际 cpal 推送的 chunk 错位.

**注意**: 这只动了 VAD 喂入策略, 不动 VAD 配置 / 触发逻辑. 如果诊断错了可以回滚.

## Risks / Trade-offs

- [R1: 改 VAD `accept_waveform` 可能让 VAD 触发行为变化, 引入新 bug] → Mitigation: 先跑 `test_vad_trigger_latency` 看延迟数字是否仍在 372ms 量级 (±100ms 可接受); 再跑 `test_recognition_no_lost_chars` 看 ≥18 字. 两项都过才接受.
- [R2: pre_roll 2000ms 增加内存 ~64KB] → Mitigation: 可忽略, 32k f32 = 128KB.
- [R3: 硬断言 ≥ 18 字未来 SenseVoice 模型升级若引入更多错字会让测试 fail] → Mitigation: 阈值可调到 17, 但当前先按 ≥18 走.
- [R4: 真实 `recognition_loop` 仍可能识别 < 18 字, 即本设计落地后未达标] → Mitigation: tasks.md 留一个 task 走二次诊断 (在 `recognition_loop` 加 buffer dump 钩子, dump 出 buffer 喂 SenseVoice 看真实结果), 不达 18 字不收.

## Migration Plan

无. 单 crate 单文件改动, 协议层不变, 服务可直接重启.

## Open Questions

- 真实 `recognition_loop` 仍可能只识别 14 字 (D1+D2+D4 都试过), tasks.md 留"二次诊断"task 兜底.
- "院→博"是 SenseVoice 模型行为还是 wav 录音问题? 暂不调查, 不在本 change 范围.