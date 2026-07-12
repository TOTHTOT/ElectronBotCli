## Why

`ServerEvent::Volume.value` 在 cpal 回调里走的是线性映射 `(peak * 100.0).min(100.0)`, cpal f32 数据范围 [-1.0, 1.0], 日常说话 peak 落在 0.05–0.3 之间, 算下来值在 5–30 飘. 渲染端 `render_volume_bar` 把 18 格内宽按 100 缩放, 数字 30 才填 5 格, 数字 10 只填 1 格, 音量条视觉幅度几乎看不出来, 用户反馈"音量条不会动, 只有数字会动". 这次归档的 `add-voice-realtime` spec 也只描述了"0..=100 归一化峰值", 没规定具体缩放方式, 实现细节任由实现侧随意选, 是 spec 不够具体的隐含问题.

## What Changes

- **服务端**: `crates/ele_bot_server/src/media/voice/asr.rs` 把 cpal 回调里的线性 `peak*100` 替换成 dB 对数刻度, 抽出纯函数 `peak_to_volume(peak: f32) -> i32`. 映射公式 `20·log10(peak)`, `-40 dB` 视为静音 (值 0), `0 dB` 视为满刻度 (值 100). 慢速衰减 (×0.95 / 32ms) 保持不变, 视觉响应曲线与之前一致.
- **服务端**: 新增 `peak_to_volume` 的 unit test (`crates/ele_bot_server/src/media/voice/asr.rs::tests`), 验证典型 peak 值映射结果.
- **spec**: 同步更新 `openspec/specs/voice-realtime/spec.md`, 在 "协议支持音量广播" requirement 里加入 "采用 dB 对数刻度" 的实现说明, 在 Scenario 显式约束 `value` 计算方式. 让 spec 从"实现可选"变成"实现有规约".

## 目标

- 修正 cpal 输入音量映射公式, 让小声音量条视觉上能稳定跳动
- 让 voice-realtime spec 显式约束音量映射方式, 防止后续实现再回退到线性缩放
- 引入可单测的纯函数 `peak_to_volume`, 提升映射逻辑的可验证性

## 非目标 (Non-goals)

- 不改变音量采样窗口 (32ms / cpal 回调频率), 不改变衰减系数 (0.95)
- 不改 `ServerEvent::Volume` 协议字段或语义 (value 仍是 0..=100)
- 不动 ASR / VAD / TTS 任何路径, 不影响识别
- 不持久化音量值 (仍是运行时数据)
- 不改设备状态页 UI 渲染逻辑 (`device_status.rs` 仍按 0..=100 缩放)

## 影响范围

- `crates/ele_bot_server/src/media/voice/asr.rs`: 抽出 `peak_to_volume`, cpal 回调调用; 新增 `peak_to_volume_mapping` 单元测试
- `openspec/specs/voice-realtime/spec.md`: 修正 "协议支持音量广播" requirement, 加入 dB 映射公式描述

## Capabilities

### New Capabilities

(无新增 capability)

### Modified Capabilities

- `voice-realtime`: "协议支持音量广播" requirement 补充音量映射公式 (dB 对数刻度, -40 dB floor), 把"0..=100 归一化峰值"从概念性描述收紧为可验证规约