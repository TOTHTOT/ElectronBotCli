# Tasks: fix-volume-meter-scale

## 1. server 层 — cpal 音量映射改为 dB 对数刻度

- [x] 1.1 在 `crates/ele_bot_server/src/media/voice/asr.rs` 抽出纯函数 `peak_to_volume(peak: f32) -> i32`, 用 `20*log10(peak)`, `-40 dB` 映射 0, `0 dB` 映射 100, 范围外 clamp. 补 `///` rustdoc 说明映射语义和 -40 dB floor 选择原因. 完成后跑三件套
- [x] 1.2 把 cpal 回调里的 `(peak * 100.0).min(100.0)` 替换成 `peak_to_volume(peak)`. 行内注释保留原本的"峰值检测 + 慢速衰减, 类似 VU 表"语义. 完成后跑三件套

## 2. server 层 — peak_to_volume 单元测试

- [x] 2.1 在 `crates/ele_bot_server/src/media/voice/asr.rs::tests` 模块新增 `peak_to_volume_mapping` 测试, 覆盖: 静音/底噪/小声 (peak=0.05)/中等 (peak=0.1)/大声 (peak=0.5)/满刻度 (peak=1.0)/超界 (peak=2.0) 七个场景. 完成后跑三件套
- [x] 2.2 单独跑 `cargo test -p ele_bot_server --lib peak_to_volume` 确认新测试通过, 不回归其他测试 (允许历史上 fixture 路径缺失导致的失败)

## 3. spec 同步

- [x] 3.1 在 `openspec/changes/fix-volume-meter-scale/specs/voice-realtime/spec.md` 写好 `## MODIFIED Requirements`, 修改 "协议支持音量广播" requirement 主体 (加入 dB 映射描述 + 衰减曲线描述) 并补充新 Scenario "dB 映射函数行为". 完成后 `openspec validate fix-volume-meter-scale` 通过