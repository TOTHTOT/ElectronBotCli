# ASR 识别丢首字 + VAD 不触发 — 根因与修复

> 写给后续维护 ASR 模块的人. 这两个 bug 都在 2026-07 修复, 但都属于
> "测试 OK / 生产挂"类型 — 单元测试跑通但实际服务端跑起来不对.
> 写下来避免踩同一个坑.

## 背景

`crates/ele_bot_server/src/media/voice/asr.rs::recognition_loop` 是服务端
ASR 主循环: 每轮从 cpal audio channel 收音频, 喂 VAD, VAD 触发时把累积
的 buffer 喂 SenseVoice 离线识别, 静音超时后切回静音段.

这条链路在生产跑 (cpal channels=2, 麦克风阵列, 立体声 → 单声道 downmix)
时发现两个独立 bug, 都让整个语音交互废掉:

1. **VAD 从不触发**: 说话时音量条跳到 30-50, 但永远没有 `>>> Speech start` 日志
2. **VAD 触发了, 识别丢前 5 字**: 触发后 SenseVoice 只返回后 14 字, 期望文本前 5 字 ("欢迎大家来体") 全部丢失

两个 bug 的根因都跟 "audio chunk 的样本数 vs 代码假设" 错位有关, 下面分别拆.

---

## Bug 1: VAD 在立体声设备下从不触发

### 现象

服务端起来后, 在 "设备状态" 页面看到 "输入音量" 进度条正常跳动 (30-50 中等),
对着麦克风说话, 但永远没有 `>>> Speech start` 日志, 没有识别结果.

### 根因

`recognition_loop` 里 VAD 喂入代码 (旧版):

```rust
let is_speech = if samples.len() >= 512 {
    vad.accept_waveform(&samples[..samples.len().min(512)]);
    vad.detected()
} else {
    false
};
```

`samples.len() >= 512` 这个守卫是**致命 bug**.

| 场景 | samples 来源 | samples.len() | ≥ 512? | VAD 喂了吗 |
|---|---|---|---|---|
| **单元测试** (`test_recognition_no_lost_chars`) | 测试代码 `for chunk in samples.chunks(chunk_size)`, chunk_size=1600 | 1600 | true | ✓ |
| **生产 cpal 流** | `process_audio_chunk` 把立体声 downmix 到单声道: `data.chunks(2).map(\|c\| (c[0]+c[1])/2.0)`, cpal Fixed(512) 立体声帧 | **256** | **false** | **✗ 永远不喂** |

生产 cpal 配置 channels=2, buffer_size=Fixed(512) — 每帧 512 立体声样本.
`process_audio_chunk` 把左右声道均值 downmix 成单声道, 512 立体声 → 256 单声道.
`samples.len() = 256 < 512` → 守卫 false → 永远走 else 分支 → VAD 永远不被喂数据 → `vad.detected()` 永远 false → VAD 永远不触发.

**为什么测试 OK**: 测试用 `chunk_size=1600` 直接喂 wav, 不经过 cpal 的立体声 downmix, samples.len()=1600 ≥ 512 → 测试通过. 这是典型的 "测试 OK / 生产挂" 案例 — 测试 fixture 跟生产路径不一致.

### 修复

去掉长度守卫, 改成"有多少喂多少":

```rust
// VAD detection: 喂当轮新收的 samples (而非 pre_roll 滑动子集),
// 让 VAD 内部状态跟 cpal 推送的 chunk 对齐.
// 注: cpal 配置 channels=2, process_audio_chunk 里立体声 downmix 到单声道
// (每 2 样本 → 1), 所以 samples.len() 通常 = 256 (32ms). 不能用
// samples.len() >= 512 守卫, 否则 VAD 永远不会被喂数据.
let feed_n = samples.len().min(VAD_WINDOW_SIZE as usize);
vad.accept_waveform(&samples[..feed_n]);
let is_speech = vad.detected();
```

`sherpa_onnx::VoiceActivityDetector` 内部有窗口缓冲, 不需要外部保证 ≥ 512 样本 — 多次喂 256 样本累积即可.

### 教训

**任何跟 cpal 音频帧长度 / 通道数耦合的守卫都要慎重**. 测试 fixture 应该模拟 cpal 的真实输出 (downmix 后单声道 256 样本), 而不是绕过它直接发 1600 样本的"理想 chunk".

---

## Bug 2: VAD 触发后 SenseVoice 识别丢前 5 字

### 现象

VAD 触发了 (有 `>>> Speech start` 日志), 也走完了完整 ASR 流程, 但
`assets/audio/asr_example_zh.wav` (预期 19 字 "欢迎大家来体验达摩院推出的语音识别模型")
识别成 14 字 "验达摩院推出的语音识别模型。", 丢了前 5 字 "欢迎大家来体".

### 根因

旧 `recognition_loop` 的 buffer 拼接逻辑:

```rust
loop {
    let samples = recv_audio();         // 1. 收 cpal 推送的 chunk
    pre_roll.extend(&samples);          // 2. pre_roll 滑动窗口装当轮

    if vad.detected() {
        if !speaking {
            speaking = true;
            buffer.extend(&pre_roll);   // 3. VAD 首次触发: buffer 复制 pre_roll
        }
        buffer.extend(&samples);        // 4. buffer 继续追加当轮 chunk
    }
    // ...
}
```

**问题在第 3 步**: `buffer.extend(&pre_roll)` 装的是 pre_roll 当前内容.
pre_roll 是 500ms 滑动窗口, 在 VAD 触发那一刻装的是 "触发点前的 500ms" 音频.
但 VAD 触发**滞后**真实语音起点约 372ms (silero_vad `detected()` 翻 true 是在
真实语音开始 372ms 之后), 加上 silero_vad 内部 600ms min_silence/min_speech 缓冲,
**pre_roll 装的 "触发点前 500ms" 实际上是 wav [812ms, 1312ms]** — wav 前 812ms
完全没进 buffer.

更糟糕的是: buffer 第 3 步装 pre_roll 后**还**装第 4 步的当轮 chunk (wav [20800, 22400ms]
= wav [1300ms, 1400ms]). pre_roll 在那一刻装的是 wav [812, 1312], 所以 buffer 第 3 步装的
是 [812, 1312] = 500ms — **跟 wav 实际位置 [812, 1312ms] 对齐**, 完整.

但 wav [0, 812ms] 永远**不进 buffer** — 因为 `speaking=false` 时 buffer 不积累.
`buffer.clear()` 之后 buffer 是空的, VAD 触发那一刻才装 pre_roll.

### 修复

把 buffer 拼接策略改成 "持续积累":

```rust
if is_speech {
    if !speaking {
        speaking = true;
        // buffer 已在 speaking=false 期间持续装 samples, 这里不再
        // extend(&pre_roll), 否则 wav 末尾 100ms 会被装两遍, SenseVoice
        // 把"体验"识别为"体验体验".
    }
    buffer.extend(&samples);
} else if speaking {
    // 静音但仍 speaking, 继续 extend 让 buffer 跟上音频流
    silence_count += 1;
    buffer.extend(&samples);
    // ...
} else {
    // speaking=false 且 is_speech=false (静音段): 仍 extend samples,
    // 让 buffer 累积 wav 头静音段. speech_start 时 buffer 已包含 wav
    // 起点, SenseVoice 看到完整时间线.
    buffer.extend(&samples);
}
```

要点:

1. **buffer 在静音段也持续装 samples** — speech_start 时 buffer 已经装了 wav 起点静音段, 不依赖 pre_roll
2. **speech_start 时不再 extend(&pre_roll)** — 否则 wav 末尾 100ms 被装两遍, SenseVoice 把 "体验" 识别为 "体验体验" (实测出现过)

修复后实测:

```
识别结果: "欢迎大家来体验达摩院推出的语音识别模型。"
共同前缀: "欢迎大家来体验达摩院推出的语音识别模型" (19 字)
```

19/19 字完整命中, "院→博" 单字错字也修正 (SenseVoice 在完整时间线下正确识别 "院").

### 为什么 500ms pre_roll 容量不够

旧设计意图是 "pre_roll 装 VAD 触发点前 N 毫秒, 弥补 VAD 触发延迟".
但实测即使把 pre_roll 扩到 2000ms 也**不能**解决问题:

- 2000ms pre_roll 装 wav [0, 2000ms] (因为 VAD 在 1312ms 触发)
- speech_start 时 buffer = wav [0, 20800ms]
- SenseVoice 看到完整时间线 ✓
- 但识别结果出现 "体验体验" 重复 (buffer 末尾 100ms 被装两遍)

根本原因不是 pre_roll 不够长, 而是 **"speech_start 时复制 pre_roll" 这个动作本身有问题** — pre_roll 在 VAD 触发那一刻已经"领先"于 samples 100ms (因为 samples 是当轮 chunk, pre_roll 已经 extend 过 samples). speech_start 装 pre_roll 又装 samples = 那 100ms 重复.

所以正确方向是**让 buffer 自己积累, 不依赖 pre_roll 触发复制**.

### 教训

**`VAD detected → 触发解码` 这条链路应该 "VAD 触发 = 切换解码标志位", 不是 "VAD 触发 = 给 buffer 补充内容"**. buffer 应该像录音机一样始终在录, 不依赖外部信号决定是否录.

---

## 顺带修复: pre_roll 滑动窗口超容量 bug

pre_roll 的 while 滑动循环旧版:

```rust
while pre_roll.len() >= PRE_ROLL_SAMPLES {
    pre_roll.pop_front();
}
pre_roll.extend(&samples);  // 假设 samples=1600, PRE_ROLL_SAMPLES=8000
```

退出 while 时 pre_roll.len() = 7999, extend 1600 → 9599 > 8000 容量上限 1599 ≈ 100ms.

修复: 一次性 pop 到位:

```rust
while pre_roll.len() + samples.len() > PRE_ROLL_SAMPLES {
    pre_roll.pop_front();
}
pre_roll.extend(&samples);
```

修复后实测 pre_roll 始终 ≤ 8000.

---

## 配套硬化

`test_recognition_no_lost_chars` 升级为硬断言 `assert!(common >= 19, ...)`, 跑测试如果识别 < 19 字立即 fail + 打印完整识别文本, 回归立刻能看到. 跑法:

```bash
cargo test --package ele_bot_server -- --ignored test_recognition_no_lost_chars --nocapture
```

---

## 时间线

| 日期 | Commit | 说明 |
|---|---|---|
| 2026-07-23 | `d841409` | buffer 持续积累 + pre_roll 滑动窗口 + VAD accept_waveform 喂当轮 samples |
| 2026-07-23 | `e17fd6a` | VAD 立体声 samples.len()=256 守卫 bug 修复 |
| 2026-07-23 | `f1489e0` | OpenSpec fix-asr-lost-prefix-words tasks + spec 对齐 |

OpenSpec: `openspec/changes/fix-asr-lost-prefix-words/` (4/4 artifacts complete)