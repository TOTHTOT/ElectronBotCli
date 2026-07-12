# Tasks: audio-device-picker

## 1. proto 层 — 协议消息

- [x] 1.1 在 `crates/ele_bot_proto/src/messages.rs` 新增 `pub struct DeviceInfoDto { name: String, display: String, driver: Option<String> }`, 带 `///` rustdoc 说明三字段用途与 `display` 拼接格式. 完成后跑三件套
- [x] 1.2 在 `ClientMessage` 新增变体 `ListInputDevices` 和 `ListOutputDevices`, 各带 `///` rustdoc. 完成后跑三件套
- [x] 1.3 在 `ServerEvent` 新增变体 `InputDevices { devices: Vec<DeviceInfoDto> }` 和 `OutputDevices { devices: Vec<DeviceInfoDto> }`, 各带 `///` rustdoc. 完成后跑三件套
- [x] 1.4 在 `messages.rs` 末尾 `mod tests` 增补 roundtrip 测试: 4 条新消息各自 serialize → deserialize 后字段一致. 完成后跑三件套

## 2. server 层 — 命令处理与热重建

- [x] 2.1 在 `crates/ele_bot_server/src/state.rs` 新增 `pub fn rebuild_voice(&self) -> anyhow::Result<()>`, 实现见 design D2: 读 config → `init_voice` → lock+replace. 补 `///` rustdoc 含边界说明 (失败时保持旧实例). 完成后跑三件套
- [x] 2.2 把 `state.rs::init_voice` 改为 `pub(crate)` 或内联进 `rebuild_voice`, 使外部 crate 不会触及, 内部可调用. 完成后跑三件套
- [x] 2.3 在 `crates/ele_bot_server/src/ws.rs::handle_command` 增加两个分支: `ClientMessage::ListInputDevices` → 调用 `voice::list_input_devices()` 转 `DeviceInfoDto` Vec 后 `out_tx.send(ServerEvent::InputDevices { devices })`; 输出设备同理. 完成后跑三件套
- [x] 2.4 在 `set_config` 路径 (现有 `ClientMessage::SetConfig` 分支) 增加检测: 若新 `config.speech_name` 或 `config.output_device` 与旧值不同, 调用 `state.rebuild_voice()`. 失败时 `out_tx.send(ServerEvent::Error { ... })`. 完成后跑三件套
- [x] 2.5 在 `state.rs` 增加辅助方法 `current_audio_config(&self) -> (String, String)` 返回当前 `(speech_name, output_device)`, 给 2.4 的对比用. 补 `///` rustdoc. 完成后跑三件套

## 3. client 层 — 网络收发

- [x] 3.1 在 `crates/ele_bot_client/src/net.rs` 增补两个 helper: `send_list_input_devices(&self)` 和 `send_list_output_devices(&self)`. 这两个函数最终都通过现有 `tx.send(ClientMessage::...)` 发出. 补 `///` rustdoc 说明调用时机. 完成后跑三件套
- [x] 3.2 在客户端 ws 接收循环处理 `ServerEvent::InputDevices` / `OutputDevices` 时, 把 `devices` 写入 `App` 缓存 (见任务 4.1). 处理 `ServerEvent::Error` 时弹一个错误 overlay 或临时状态 (与现有错误处理一致)

## 4. client 层 — App 状态

- [x] 4.1 在 `crates/ele_bot_client/src/app/mod.rs` 的 `App` 结构体新增字段:
  - `pub input_devices: Vec<DeviceInfoDto>`
  - `pub output_devices: Vec<DeviceInfoDto>`
  - `pub device_list_loading: bool` (用于 picker 进入时显示"加载中")

  补字段注释说明用途与生命周期. 初始化为 Vec::new() / false. 完成后跑三件套
- [x] 4.2 新增 `App` 方法 `selected_input_index(&self) -> usize` 和 `selected_output_index(&self) -> usize`, 返回当前 `speech_name` / `output_device` 在 (系统默认 + 缓存列表) 中的索引 (找不到则 0). 补 `///` rustdoc. 完成后跑三件套
- [x] 4.3 新增 `App` 方法 `set_input_device(&mut self, name: String)` / `set_output_device(&mut self, name: String)`, 只更新本地 config 字段. 实际发送走 `net.send_set_config` (沿用现有 SetConfig 路径). 完成后跑三件套

## 5. client 层 — Route 子模式

- [x] 5.1 在 `crates/ele_bot_client/src/app/route.rs` 新增枚举 `pub enum DeviceKind { Mic, Speaker }` (带 `///` rustdoc). 完成后跑三件套
- [x] 5.2 给 `Route::Settings` 增加字段 `selecting: Option<DeviceKind>`. 更新 `From<MenuItem>` 默认值 (进入 Settings 时 `selecting: None`). 完成后跑三件套

## 6. client 层 — ViewModel & 渲染

- [x] 6.1 在 `crates/ele_bot_client/src/ui/viewmodel/settings.rs` 的 `SettingsViewModel` 增加字段:
  - `pub input_devices: Vec<DeviceInfoDto>`
  - `pub output_devices: Vec<DeviceInfoDto>`
  - `pub device_list_loading: bool`

  在 `from_app` 里从 `app` 取值填充. 完成后跑三件套
- [x] 6.2 给 `SettingsViewModel` 增加方法 `pub fn picker_items(&self, kind: DeviceKind) -> Vec<(String, String)>` 返回 `(name, display)` 对, 首项固定为 `("", "<系统默认>")`, 后接缓存列表. 补 `///` rustdoc. 完成后跑三件套
- [x] 6.3 在 `crates/ele_bot_client/src/ui/pages/settings.rs` 重构 `render`: 根据 `vm.selecting` 切换两种布局:
  - `selecting = None`: 现有列表渲染, 但麦克风/扬声器两行的 value 改为显示"当前选中项的 display" (调 `vm.picker_items(kind)` 找到当前 index)
  - `selecting = Some(kind)`: 在原区域渲染单列 picker, ↑/↓ 高亮, 首/末项可见; 加载中或空列表显示对应占位
- [x] 6.4 更新 `render_info_bar` 在 `selecting = Some(_)` 时显示 `操作: [↑/↓] 选择  [Enter] 确认  [Esc] 取消  [R] 刷新`. 完成后跑三件套

## 7. client 层 — 输入派发

- [x] 7.1 在 `crates/ele_bot_client/src/input/settings.rs` 的 `SettingsEvent` 枚举新增变体:
  - `EnterPicker` — 在麦克风/扬声器行按 Enter 时触发
  - `PickerUp` / `PickerDown` — picker 内移动
  - `PickerConfirm` — picker 内按 Enter 确认
  - `PickerCancel` — picker 内按 Esc 取消
  - `RefreshDevices` — 列表模式按 R

  每个变体带 `///` rustdoc 说明触发条件. 完成后跑三件套
- [x] 7.2 在 `settings::handle` 增加对应分支:
  - `EnterPicker`: 根据 `app.ui.mode.route` 当前 selected index 决定 Mic (假设 index 2) 还是 Speaker (假设 index 3), 切换到 `Route::Settings { selecting: Some(kind), .. }`, 然后调 `app.refresh_audio_devices()` 发 List* 消息
  - `PickerUp` / `PickerDown`: 修改 picker 内的 index 字段 (见 7.3)
  - `PickerConfirm`: 调 `app.set_input_device(name)` 或 `set_output_device(name)`, 然后清 `selecting`
  - `PickerCancel`: 只清 `selecting`
  - `RefreshDevices`: 调 `app.refresh_audio_devices()`

  所有分支通过 `handle_event` 派发, 不绕过. 完成后跑三件套
- [x] 7.3 给 `Route::Settings` 或新建辅助结构, 持久化 picker 内的 `picker_index: usize`. Esc/确认后清零. 完成后跑三件套
- [x] 7.4 在 `crates/ele_bot_client/src/input/mod.rs::handle_settings` 把麦克风/扬声器行 (selected 2 / 3) 的 Enter 翻译为 `SettingsEvent::EnterPicker`, 其它行继续走 `SettingsEvent::Enter` 进入 `EditField`. 完成后跑三件套

## 8. 端到端验证

- [x] 8.1 手动验证脚本: 启动 server + client, 进入设置页, 麦克风/扬声器显示设备列表 (含 driver), 选非默认项后 TTS 能从新扬声器出声, ASR 能识别新麦克风; 选系统默认后回退到 host default. 记录到 PR 描述
- [x] 8.2 跑 `cargo fmt --all && cargo clippy --all-features --all-targets -- -D warnings && cargo check --all-features --all-targets`, 全过
- [x] 8.3 跑 `cargo test -p ele_bot_proto`, 消息 roundtrip 全过