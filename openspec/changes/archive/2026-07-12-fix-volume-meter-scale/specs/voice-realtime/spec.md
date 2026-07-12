## MODIFIED Requirements

### Requirement: 协议支持音量广播
系统 SHALL 提供一个 `ServerEvent::Volume { value: i32 }` 变体, 承载当前麦克风输入的归一化峰值音量 (0..=100). 客户端不需要单独请求; 字段值在 voice 不可用时为 0.

音量 `value` SHALL 通过 dB 对数刻度从 cpal f32 峰值样本映射得出: 取一个音频回调窗口内样本绝对值的最大值作为 `peak` (0.0..=1.0), 计算 `db = 20 * log10(peak)`, 再线性映射 `[-40 dB, 0 dB]` 到 `[0, 100]`, 超出范围 SHALL 被 clamp. `-40 dB` 是常见麦克风本底噪声量级, 低于此值 SHALL 输出 0. 这样小声音量 (peak 0.05–0.3) 在音量条上能稳定覆盖 30–70 的可见范围, 不退化成 1–3 格. 系统 SHALL 同时对音量做慢速指数衰减 (每 32ms ×0.95), 视觉上呈 VU 表响应特性.

#### Scenario: 服务端广播音量
- **WHEN** 服务端持有 `Arc<VoiceManager>` 且 cpal 输入流在产生数据
- **THEN** 服务端 SHALL 每 50ms (20Hz) 通过 `event_tx` 广播一次 `ServerEvent::Volume`, `value` 等于 `voice.volume().load(Relaxed)`
- **AND** voice 不可用 (`state.voice` 为 `None`) 时, 广播的 `value` SHALL 为 0
- **AND** `value` SHALL 由 dB 映射函数 `peak_to_volume(peak)` 计算得出, 不使用线性 `peak*100`

#### Scenario: 客户端接收并缓存
- **WHEN** 客户端 `apply_event` 收到 `ServerEvent::Volume { value }`
- **THEN** 客户端 SHALL 将 `server.volume` 字段更新为 `value`
- **AND** 现有 `device_status.rs` 渲染逻辑 SHALL 无需改动即可看到音量条随说话变化

#### Scenario: dB 映射函数行为
- **WHEN** cpal 回调读到静音 (peak = 0.0)
- **THEN** `peak_to_volume` SHALL 返回 0
- **WHEN** cpal 回调读到小声音量 (peak = 0.05, ≈ -26 dB)
- **THEN** `peak_to_volume` SHALL 返回 30..=40 之间的整数值
- **WHEN** cpal 回调读到中等音量 (peak = 0.1, -20 dB)
- **THEN** `peak_to_volume` SHALL 返回 50..=55 之间的整数值
- **WHEN** cpal 回调读到满刻度 (peak = 1.0, 0 dB)
- **THEN** `peak_to_volume` SHALL 返回 100
- **WHEN** cpal 回调读到超过 1.0 的异常值
- **THEN** `peak_to_volume` SHALL clamp 到 100