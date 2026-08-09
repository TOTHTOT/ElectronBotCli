# Contract: 协议变更（proto）

宪法 IV：只增不改。本特性协议面变更如下。

## 1. `AppConfig` 新增字段（types.rs）

```toml
# config.toml 示例 (新字段缺省时 serde default 100)
speaker_volume = 80   # 扬声器增益 [0, 100], 默认 100
mic_volume = 60       # 麦克风增益 [0, 100], 默认 100
```

- 序列化：两字段均带 `#[serde(default = "default_100")]`，**不带**
  `skip_serializing_if`（全量回写模型，始终序列化）。
- 兼容性矩阵：

| 组合 | 行为 |
|------|------|
| 新 client → 新 server | 全功能 |
| 旧 client → 新 server | 正常；旧 client 回写 SetConfig 会把音量重置为 100（已知限制，见 data-model.md） |
| 新 client → 旧 server | 旧 server serde 忽略未知字段，不报错；音量调节无效但不崩溃 |
| 旧 config.toml → 新 server | 缺字段 default 100，行为与升级前一致 |

## 2. `ClientMessage::SetConfig`（无结构变更）

音量调节复用现有全量 `SetConfig` 消息。server 端语义增量：

- `speaker_volume` / `mic_volume` 变化 → 热更新增益原子量 + 持久化，
  **不触发** `rebuild_voice`
- 其他字段语义不变

## 3. `ServerEvent::Volume { value: i32 }`（零变更，复用）

实时输入电平 [0, 100]，已存在并在广播。本特性仅新增一个消费点
（设置页麦克风音量条），协议本身不动。电平值为**增益后**信号的峰值
（attack/decay 平滑 + 节流策略不变）。

## 4. 测试要求

- proto：旧 TOML（无新字段）→ `AppConfig::default()` 等价（100/100）；
  新旧字段 JSON roundtrip；非法值 clamp（server 侧单测）
