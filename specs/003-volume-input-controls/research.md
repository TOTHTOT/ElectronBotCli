# Research: 设置页音量调节与测试输入框优化

Phase 0 调研结论。所有 Technical Context 未知项已解决，无新增第三方依赖。

## 决策 1: 扬声器音量 = rodio `Player::set_volume` 软件增益

- **Decision**: `TtsPlayer` 持有 `Arc<AtomicU32>`（f32 增益 bits，0.0–1.0），
  `play()` / `start_streaming()` 创建 `Player` 后立即 `player.set_volume(gain)`。
- **Rationale**: rodio 0.22.2 `Player` 自带 `set_volume(Float)`（player.rs:178），
  软件增益天然跨平台（macOS/Windows/Linux/ARM 全适用）；增益存原子量，
  `SetConfig` 改音量时**不需要重建音频流**，下一次播放即生效，满足 < 200ms。
- **Alternatives considered**:
  - ALSA `amixer` 系统音量：Linux-only，违反跨平台约束，否决
  - rodio `source::amplify` 包源：每个源创建时定型，无法运行中调整，且要包
    两条路径（整段 + 流式 queue），不如 Player 级统一
  - cpal 级音量：cpal 无音量 API，否决

## 决策 2: 麦克风增益 = 采集回调内软件乘法

- **Decision**: `build_asr_stream` 新增 `gain: Arc<AtomicU32>` 参数；
  `process_audio_chunk` 在转 f32 后逐样本乘增益并 clamp 到 [-1.0, 1.0]，
  再做峰值/downmix。**电平显示基于增益后信号**，用户调增益立刻看到电平变化。
- **Rationale**: 与决策 1 同构（软件增益 + 原子量热更新），跨平台；增益后
  计算电平符合"调节立即可见效果"的 spec 要求（FR-004）。clamp 防止 >100%
  增益削波产生爆音。
- **Alternatives considered**:
  - cpal/ALSA 硬件增益：平台 API 不统一（ALSA mixer、WASAPI 无统一接口），否决
  - 在识别线程乘增益：电平显示（采集回调侧）就看不到调节效果，否决

## 决策 3: 持久化与协议 = AppConfig 增量字段 + 现有 SetConfig 链路

- **Decision**: `AppConfig` 新增 `speaker_volume: u8` / `mic_volume: u8`，
  均 `#[serde(default = "default_100")]`。client 音量条 ←→ 调节时把 mirror 的
  config 改值后整体 `SetConfig` 回写；server `set_config` 比对音量字段，
  只更新原子量 + `cfg.save()`，**不触发** `rebuild_voice`。
  麦克风实时电平**零协议改动**：`ServerEvent::Volume` 已在广播
  （asr.rs → BusEvent::Volume → ws.rs:74），client `app/mod.rs:313` 已接收
  存入 `server.volume`，设置页直接渲染。
- **Rationale**: 完全复用现有链路与持久化机制，满足宪法 IV「只增不改」；
  旧 config.toml 无新字段时 serde default 100 = 不改变现有行为。
- **已知限制**（不违反宪法 IV，记录在案）：旧版本 client 回写 `SetConfig`
  时新字段丢失 → serde default 回填 100，会把已调音量重置。无法区分
  "旧 client"与"显式设 100%"，接受该降级（不报错、功能可用）。
- **Alternatives considered**:
  - 新增 `SetVolume` 专用消息：与 SetConfig 全量模型重复，且仍解决不了旧
    client 回写覆盖问题，否决
  - client 本地另存音量副本：违反"server 是唯一权威源"，双写必漂移，否决

## 决策 4: 输入框 = 新建 `ui_components::TextInput`，不动 route.rs `EditField`

- **Decision**: 在 `crates/ele_bot_client/src/ui_components/text_input.rs` 新建
  `TextInput`：`buffer: String` + `cursor: usize`（char 索引），方法
  `insert_char / delete_back / delete_forward / move_left / move_right /
  move_to_start / move_to_end / clear / before_cursor / after_cursor`。
  TTS/LLM 测试页 state 用它替换裸 `input_text: String`；渲染沿用设置页
  已验证的**块字符 caret** 策略（settings.rs:96 注释记录了为何不用终端原生
  光标 — popup Clear 会抹掉），超宽时按 caret 位置横向滚动窗口。
  route.rs `EditField` **保持不动**。
- **Rationale**: `EditField`（route.rs:75）的编辑逻辑与要新建的核心等价，
  但它耦合 overlay 语义（`index`/`label`/菜单路由），直接复用会把 overlay
  概念泄进测试页；把它重构出来又要动已工作的设置 overlay 输入路径，风险
  大于收益。两个消费者（测试页 ×2）+ 未来候选（EditField）共享一份
  char 级编辑核心不算过早抽象。
- **char 级 vs grapheme 级**: char 级删除/移动对中文完全正确（无乱码、无半字），
  与现有 `EditField` 行为一致；emoji 组合序列（如 👨‍👩‍👧）需多次退格删净，
  不产生乱码。引入 `unicode-segmentation` 可做到 grapheme 级，但收益仅限
  emoji 组合字符，违反宪法 V 最小依赖，**决定不引入**，spec edge case 中
  "按字符而非字节"的表述 char 级已满足。
- **Alternatives considered**:
  - `tui-textarea` crate：功能全但引入新依赖 + 一套新的事件/渲染模型，
    与现有派发契约（宪法 I）和 caret 渲染策略都要适配，成本远高于 60 行
    的 TextInput，否决
  - 直接复用 `EditField`：overlay 语义泄漏，见上，否决

## 决策 5: 设置页音量条交互 = 选中即调，无编辑态

- **Decision**: 设置列表新增两行（扬声器音量 / 麦克风音量，插在输出设备与
  TTS 配置之间）。选中音量行时 ←→ 直接 ±5%（0/100 边界钳位），每次变化
  立即用 config mirror 回写 `SetConfig`；不需要 Enter 进入编辑态。
  渲染：增益条（复用 device_status.rs `render_bar` 风格）+ 百分比；
  麦克风行在增益条上叠加实时电平指示（`server.volume`）。
- **Rationale**: 音量是连续量，←→ 直调比 Enter+输入数字 符合直觉；spec
  验收场景 1/2 即"选中音量条并按右方向键"。操作说明栏对音量行动态提示。
- **Alternatives considered**:
  - 复用 EditField 输入数字：连续量用文本输入反直觉，否决
  - 调节防抖后发送：键盘步进频率低（人按 ←→ ≈ 每秒几次），SetConfig 全量
    也很小，直接发即可，防抖是无谓复杂度
