# enhance-device-picker

## 为什么

`audio-device-picker` change 已归档, 主 spec 落地在 `openspec/specs/audio-device-picker/spec.md`, 描述了完整的设备选择体验. 但**实装从未进 main**:

- 协议层 (`proto/src/{types.rs, messages.rs}`) 没有 `DeviceInfoDto` / `ListInputDevices` / `InputDevices` 等类型
- 服务端 `voice::list_*_devices()` (在 `voice-realtime` change 中实装) 未被 ws 层暴露
- 客户端 Settings 页仍是 3 个文本框 (`Wifi 名称` / `Wifi 密码` / `麦克风名称`), 没有 picker UI, 没有 `selecting` 子状态

同期的 `voice-realtime` change 已经把 `VoiceManager` 热重建 (50ms 软替换) 实装进了 main. `set_config` 检测 `speech_name` / `output_device` 字段变化会自动触发 `rebuild_voice()`. "保存即切换" 在服务端已具备条件, 缺的只是前端路径.

本 change 把"补回"与"上次未明说的小增强"一次性收口.

## 改什么

### 协议层 (`ele_bot_proto`)

新增 5 个类型:

```rust
pub struct DeviceInfoDto {
    pub name: String,            // exact match with server cpal name
    pub display: String,         // "WASAPI 麦克风阵列 (2ch / 48000Hz)"
    pub driver: Option<String>,  // 独立字段, 便于客户端按需渲染
    pub channels: u16,
    pub sample_rate: u32,
}

// 新增 ClientMessage 变体
ListInputDevices,
ListOutputDevices,

// 新增 ServerEvent 变体
InputDevices { devices: Vec<DeviceInfoDto> },
OutputDevices { devices: Vec<DeviceInfoDto> },
```

`driver` 作为独立字段, 而不是仅隐藏在 `display` 字符串里 (理由见 `design.md` D1).

### 服务端 (`ele_bot_server`)

- `voice/mod.rs`: 新增 `pub fn list_input_devices_dto() -> Vec<DeviceInfoDto>` 与 `list_output_devices_dto()`, 把内部 `DeviceInfo` 转 DTO.
- `ws.rs`: 处理 `ClientMessage::ListInputDevices` / `ListOutputDevices`, 调用对应函数, 返回 `ServerEvent::{Input,Output}Devices { devices }`. 异常路径返回 `ServerEvent::Error { message }`.

### 客户端 (`ele_bot_client`)

- **状态**:
  - `App.devices: DeviceCache { inputs: Vec<DeviceInfoDto>, outputs: Vec<DeviceInfoDto>, loaded_at: Instant }` —— 跨 Route 共享.
  - `App.last_device_submit: Option<Instant>` + `App.last_device_submit_kind: Option<DeviceKind>` —— 失败 UX 留痕.

- **Route 扩展** (与现有 `editing: Option<EditField>` 对称):
  ```rust
  Route::Settings {
      selected: usize,
      editing: Option<EditField>,
      selecting: Option<SelectingField>,  // 新增
  }
  ```
  `SelectingField { kind: SelectingKind (Input | Output), cursor: usize, loading: bool }`.

- **Overlay 新变体** (与 `Overlay::EditField` 平行):
  - `Overlay::DevicePicker(SelectingField, Vec<DeviceInfoDto>)` —— picker 主体
  - `Overlay::DeviceSwitchFailure { kind: DeviceKind, old_device_name: String }` —— rebuild 失败提示 (E3 = c)

- **`SettingsEvent` 新增 6 变体**: `EnterPicker`, `PickerUp`, `PickerDown`, `PickerConfirm`, `PickerCancel`, `RefreshList`.

- **`handle_settings`** 按状态分支:
  - `selecting.is_some()` → Up/Down/Enter/Esc/'r' 翻译成 `Picker*`
  - `editing.is_some()` → 维持现状, 走 overlay 通道
  - 列表模式 → Enter on 麦克风/扬声器行 → `EnterPicker`; 'r' → `RefreshList`

- **`handle_overlay`** 加 2 个新分支:
  - `Overlay::DevicePicker` 处理 Up/Down/Enter/Esc/'r'
  - `Overlay::DeviceSwitchFailure` 处理 Esc 关闭

- **提交路径** (`PickerConfirm`):
  1. 更新 `app.config.speech_name` (输入) 或 `output_device` (输出) 为所选项 `name` (idx 0 → `""`, 即系统默认)
  2. 设 `last_device_submit = Some(Instant::now())` + `last_device_submit_kind`
  3. 关闭 picker (`selecting = None`, `overlay = None`)
  4. 发 `ClientMessage::SetConfig { config: app.config.clone() }`
  5. **不写盘** (`AppConfig::save()`)

- **ws 客户端**处理 3 个事件:
  - `ServerEvent::InputDevices { devices }` → 写 `app.devices.inputs`. 若 `selecting.kind == Input && selecting.loading` → 取消 loading 状态
  - `OutputDevices` 对称
  - `ServerEvent::Error { .. }` → 若 `last_device_submit` 在 1 秒窗口内 → 弹 `Overlay::DeviceSwitchFailure`

### UI 渲染

- **列表模式**: Settings 4 项 (改现有 3 项为 4 项, 新增"扬声器"行). 每行 `<label>: <当前选择的 display or 系统默认>`.

- **Picker overlay** (中央弹窗):
  - 标题: `选择麦克风` / `选择扬声器`
  - 单列:
    - idx 0: `> 系统默认` / `  系统默认`
    - idx 1..: `<driver dim gray> <name 亮色> (Nch / Hz dim)`
  - loading: 一行 `<加载中...>`, 屏蔽 Enter
  - 空列表: 一行 `<无可用设备>`, 屏蔽 Enter
  - Esc 始终可用

- **Failure transient overlay**:
  - 标题: `设备切换失败`
  - 内容: `已保留 <旧设备名>\n<错误明细>`
  - 自动 5 秒关 或 Esc 关

## 不做什么

- **视频设备选择** —— 仅音频 (摄像头字段保留在 AppConfig 里, 不在本 change 动)
- **设备热插拔主动监听** —— 仅手动按 R / 进 Settings 刷新
- **切换成功提示音 / TUI flash** —— 用户明确不要
- **立即写盘** —— `AppConfig::save()` 维持现状调用点
- **改 ws 鉴权 / 心跳协议**
- **改 `VoiceManager` 热重建语义** —— 已在 main, 由 `voice-realtime` 实装, 稳定

## 影响文件

| 路径 | 动作 |
|---|---|
| `crates/ele_bot_proto/src/types.rs` | 加 `DeviceInfoDto` |
| `crates/ele_bot_proto/src/messages.rs` | 加 4 个消息变体 + round-trip 测试 |
| `crates/ele_bot_server/src/voice/mod.rs` | 加 `list_*_devices_dto()`, rustdoc |
| `crates/ele_bot_server/src/ws.rs` | 处理 `List*Devices` 两条消息 |
| `crates/ele_bot_client/src/app/mod.rs` | 加 `DeviceCache` + `last_device_submit*` 字段 |
| `crates/ele_bot_client/src/app/route.rs` | `Route::Settings` 加 `selecting`, 新 `SelectingField` 类型 |
| `crates/ele_bot_client/src/app/overlay.rs` | 新 overlay 变体 `DevicePicker` + `DeviceSwitchFailure` |
| `crates/ele_bot_client/src/input/settings.rs` | `SettingsEvent` 加 6 个变体, `settings::handle` 分发 |
| `crates/ele_bot_client/src/input/mod.rs` | `handle_settings` + `handle_overlay` 分支 |
| `crates/ele_bot_client/src/ws/{client,mod}.rs` | 处理 `*Devices` + `Error` 事件 |
| `crates/ele_bot_client/src/ui/viewmodel/settings.rs` | 加 selecting / devices view-model 字段 |
| `crates/ele_bot_client/src/ui/{pages,components,...}/settings*.rs` | 新增 / 改造 picker overlay 渲染 |

实施严格按 `tasks.md` 顺序, 每完成一步跑 `cargo fmt --all && cargo clippy --all-features --all-targets -- -D warnings && cargo check --all-features --all-targets`.

## 验收

按 spec Δ 里的 scenarios 跑:

1. TUI 进设置页 → 自动收 `InputDevices` / `OutputDevices` 两条事件, 渲染 `<系统默认>` + 真实设备
2. 进麦克风选择 → ↑↓ 移动, Enter 提交, Esc 取消, R 刷新 (loading → 恢复 cursor / 回退 idx 0)
3. 提交后服务端真实切换 (`TaskStop` 旧 VoiceManager, 建新), 无 ws 重连 / 服务进程重启
4. 服务端 `rebuild_voice` 失败 → 客户端 transient overlay `设备切换失败,已保留 <旧设备>`
5. 设备名含 `<驱动名> <设备名> (Nch / Hz)` 视觉对齐 (driver dim gray, name 亮)

## 归档时

`openspec sync-specs enhance-device-picker` 把 spec Δ 合入主 spec, 再 `openspec archive 2026-07-21-enhance-device-picker`.
