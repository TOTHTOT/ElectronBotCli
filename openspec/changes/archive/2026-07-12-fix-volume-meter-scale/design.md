## Context

`add-voice-realtime` 已归档, 实现了 `ServerEvent::Volume` 50ms 周期广播 + 客户端镜像写 `server.volume` + 设备状态页音量条渲染. 但音量映射公式 (`cpal 回调 peak * 100`) 是实现侧随意选的, 没在 spec 里约束, 导致小声音量 (peak 0.05–0.3) 时音量条退化成 1–3 格, 视觉上"音量条不会动". 这次要把映射从线性换成 dB 对数, 并把规约写进 voice-realtime spec, 让后续实现不能再回退.

约束:
- 协议层 `ServerEvent::Volume { value: i32 }` 不动, `value` 仍是 0..=100
- 客户端 `device_status.rs` 渲染逻辑不动 (它已经是按 0..=100 缩放的)
- 衰减逻辑 (×0.95 / 32ms) 不动, 视觉响应曲线与之前一致

## Goals / Non-Goals

**Goals:**
- cpal 回调从线性 `peak*100` 改为 dB 对数刻度, 抽出纯函数 `peak_to_volume(peak: f32) -> i32`
- 在 spec 里加入 "采用 dB 对数刻度, -40 dB floor" 的实现约束
- 加 unit test 锁定映射行为, 防止后续实现再回退到线性缩放

**Non-Goals:**
- 不改音量采样窗口 (32ms / cpal 回调频率), 不改衰减系数 (0.95)
- 不改协议字段, 不改客户端 UI
- 不改 ASR / VAD / TTS / VoiceManager 生命周期

## Decisions

### D1: 抽出纯函数 `peak_to_volume`

```rust,ignore
/// 把 cpal f32 峰值样本 (0.0..=1.0) 映射成 0..=100 的归一化音量.
///
/// 用 dB 对数刻度: 0 dB (peak=1.0) -> 100, -40 dB (peak=0.01) -> 0.
/// 这样小声说话 (peak 0.05–0.3) 能稳定显示在 30–70, 不再退化成 1–3 格
/// 音量条. -40 dB 是常见麦克风本底噪声量级, 低于此值视为静音.
fn peak_to_volume(peak: f32) -> i32 {
    if peak <= 0.0 {
        return 0;
    }
    let db = 20.0 * peak.log10();
    (((db + 40.0) * (100.0 / 40.0)) as i32).clamp(0, 100)
}
```

**为什么不直接 inline**: 这是核心映射逻辑, 又是 spec 约束的对象, 必须能单测. 抽出后 cpal 回调只调一行 `peak_to_volume(peak)`, 行为变更可追溯.

**为什么 floor 用 -40 dB**: 常见消费级麦克风本底噪声在 -50 ~ -60 dB, -40 dB 是"略高于噪声但仍可视为静音"的合理阈值. 留 6 dB 余量避免底噪让音量条持续小幅跳动.

### D2: 衰减逻辑保持不变

```rust,ignore
let current = volume_clone.load(Ordering::Relaxed);
let new_value = if peak_value > current {
    // 新峰值: 立即提升
    peak_value
} else if current > 0 {
    // 慢速指数衰减 (约 0.95 / 32ms, 半衰期约 0.4s)
    ((current as f32) * 0.95) as i32
} else {
    0
};
volume_clone.store(new_value, Ordering::Relaxed);
```

衰减只动 `current` (AtomicI32), 不动 `peak_value` (新计算结果). 把 `peak_to_volume` 插进去, 衰减链路完全不变.

### D3: spec 同步改写 "协议支持音量广播" requirement

在 `openspec/specs/voice-realtime/spec.md` 把 requirement 主体加上 "音量 value SHALL 通过 dB 对数刻度从 cpal f32 峰值样本映射得出" 的实现约束, 并新增 "dB 映射函数行为" Scenario 锁定关键输入点的输出. 这样 archive 同步时, 主 spec 也会带上这条约束.

**为什么用 MODIFIED 而不是新增**: 这条 requirement 的语义没变 (仍是 0..=100 归一化音量), 只是把实现约束写得更紧. 现有 Scenario "服务端广播音量" 仍适用, 只是加一条 `AND value SHALL 由 peak_to_volume 计算得出`. 新 Scenario "dB 映射函数行为" 是补充, 不重复.

## Risks / Trade-offs

**[R1] dB 映射在大声时容易顶满 100** — peak=0.5 已映射到 85, peak=0.3 已 70. 大声说话可能让音量条瞬间顶满, 视觉上不像 VU 表有渐进感.
→ Mitigation: 当前设备状态页目的是"看到说话时音量条会动", 顶满是次要问题. 真要渐进感后续可加 ASYMMETRIC 压缩.

**[R2] peak=0 时退化为 0 的早返回路径可能让麦克风硬件"接近 0 但有底噪"被吞掉** — cpal 数据如果有微小噪声 (1e-5 量级), peak 不为 0, 走 dB 路径得到 -100 dB, clamp 后仍是 0. 行为正确, 但分支多了一个.
→ Mitigation: 注释清楚 `peak <= 0.0 -> 0` 是给全 0 数据的早返回, 不是去抖动.

**[R3] 改了 spec, archive 时主 spec 同步走 `## MODIFIED Requirements`, 旧的 `## Requirements` 全集会被替换** — 确认无误, 这是 archive 的预期行为.
→ Mitigation: 同步前用 `openspec show voice-realtime` 校对 diff.

## Migration Plan

无. 增量改动, 不改协议, 不改持久化. 单 commit 可 revert.