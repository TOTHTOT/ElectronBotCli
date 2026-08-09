# Data Model: 设置页音量调节与测试输入框优化

## 实体与字段

### AppConfig（proto/types.rs，增量扩展）

| 字段 | 类型 | 默认 | 说明 |
|------|------|------|------|
| `speaker_volume` | `u8` | `100` | 扬声器播放增益百分比 [0, 100]，映射 rodio `Player::set_volume(v/100)` |
| `mic_volume` | `u8` | `100` | 麦克风采集增益百分比 [0, 100]，映射采集样本乘法因子 v/100 |

- 两字段均 `#[serde(default = "default_100")]`：旧 config.toml 缺字段 → 100
  （行为不变）；旧 client 反序列化忽略新字段。
- **已知限制**：旧版本 client 回写 `SetConfig`（全量 AppConfig）时新字段按
  default 100 回填，会把已调音量重置为 100。新旧交叉不报错，仅功能降级
  （宪法 IV 允许范围内）。
- 校验：server `set_config` 时 clamp 到 [0, 100]（防御非 client 来源的
  非法值）；映射增益 = value as f32 / 100.0。

### 运行时音量状态（server，不持久化）

| 持有方 | 类型 | 说明 |
|--------|------|------|
| `VoiceManager.speaker_gain` | `Arc<AtomicU32>` | f32 bits，0.0–1.0；`TtsPlayer` 创建 Player 时读取并 `set_volume` |
| `VoiceManager.mic_gain` | `Arc<AtomicU32>` | f32 bits，0.0–1.0；`build_asr_stream` 闭包内逐样本乘算 |

- 初始化：`VoiceManager::new` 从 `AppConfig` 读初值。
- 更新：`SharedState::set_config` 检测音量字段变化 → 调
  `VoiceManager::set_speaker_gain / set_mic_gain` 写原子量。
  **不触发** `rebuild_voice`（原子量热更新，无需重建流）。
- 线程安全：`Relaxed` 序足够（标量增益，无顺序依赖）。

### 实时电平（现有链路，零改动）

`asr.rs process_audio_chunk`（增益后信号）→ `BusEvent::Volume(i32)` →
ws.rs → `ServerEvent::Volume { value }` → client `server.volume: i32`（已有）。
设置页麦克风行渲染时读取。

### TextInput（client ui_components，新建）

| 字段 | 类型 | 说明 |
|------|------|------|
| `buffer` | `String` | 输入文本 |
| `cursor` | `usize` | char 索引（非 byte），0..=字符数 |

- 不变量：`cursor` 恒在 [0, char 数] 内；所有编辑方法维护该不变量。
- char 级语义：中文删除/移动正确；emoji 组合序列需多次退格（已记录限制，
  见 research.md 决策 4）。
- 测试页 state 迁移：`TtsTestState.input_text: String` →
  `input: TextInput`（`LlmTestState` 同）；提交时取 `buffer` 全文。
- 生命周期：随页面 state，不持久化（现状不变）。

## 状态流转

### 音量调节（设置页）

```text
client 选中音量行, ←→ 按键
  → 本地 config mirror 改 speaker_volume/mic_volume (±5, clamp [0,100])
  → 发 ClientMessage::SetConfig (全量)
  → server set_config: 音量字段变化 → 写增益原子量 + cfg.save() → 广播 Config
  → client 收 Config 事件更新 mirror (渲染值与 server 权威值一致)
```

失败路径：连接断开时发送失败 → client 显示现有失败 overlay（复用
`FailureVm`），本地 mirror 不回滚（下次连上后以 server 广播的 Config 为准）。

### 测试输入框（TTS/LLM 页）

```text
按键 → input/mod.rs 派发 → input/tts_test.rs|llm_test.rs
  → TextInput 编辑方法 (Char/Backspace/Delete/←→/Home/End/Ctrl+U)
  → 渲染层按 cursor 拆 before/caret/after 三段 + 横向滚动窗口
  → Enter: 提交 buffer 全文 (沿用现有 speak_tts / send_llm_text)
```
