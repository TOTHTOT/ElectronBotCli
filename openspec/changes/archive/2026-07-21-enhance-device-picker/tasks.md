# Tasks

实施顺序严格按本章, 每完成一步跑三件套 (格式 / clippy / check):
```
cargo fmt --all && \
  cargo clippy --all-features --all-targets -- -D warnings && \
  cargo check --all-features --all-targets
```

每条任务完成时, 在 `- [ ]` 改为 `- [x]`.

## 0. 前置 (无代码改动)

- [ ] 0.1 确认 git 工作区干净, `git status` 检查
- [ ] 0.2 拉取 main 最新, `git pull --ff-only` 或确认本地已最新
- [ ] 0.3 跑一遍三件套作为 baseline, 确认 main 无警告

## 1. proto 层

- [ ] 1.1 `crates/ele_bot_proto/src/types.rs`: 加 `DeviceInfoDto { name, display, driver, channels, sample_rate }`, 带 `///` rustdoc 说明字段语义 (尤其 `name` 是 exact match key)
- [ ] 1.2 `crates/ele_bot_proto/src/messages.rs`: 加 `ClientMessage::ListInputDevices` / `ListOutputDevices` 与 `ServerEvent::InputDevices { devices }` / `OutputDevices { devices }`
- [ ] 1.3 同一个文件的 `#[cfg(test)] mod tests` 加 round-trip 测试 4 个新变体 (input list, output list, input devices, output devices)
- [ ] 1.4 跑三件套

## 2. 服务端 voice 模块

- [ ] 2.1 `crates/ele_bot_server/src/voice/mod.rs`: 加 `pub fn list_input_devices_dto() -> Vec<DeviceInfoDto>`, 把 `list_input_devices()` 现有返回的 `DeviceInfo` 序列化为 DTO. 加 `///` rustdoc 说明 wire 序列化语义
- [ ] 2.2 加 `pub fn list_output_devices_dto()` 同上
- [ ] 2.3 跑三件套

## 3. 服务端 ws

- [ ] 3.1 `crates/ele_bot_server/src/ws.rs` 在 `ClientMessage` 分发加 2 个分支: `ListInputDevices` 与 `ListOutputDevices`, 分别调 `voice::list_*_devices_dto()` 并 `out_tx.send(ServerEvent::{Input,Output}Devices { devices })`
- [ ] 3.2 在 2 个新分支的异常路径 (`voice::list_*` 返回 Err) 广播 `ServerEvent::Error { message }`
- [ ] 3.3 跑三件套

## 4. 客户端 App / Route / Overlay 状态

- [ ] 4.1 `crates/ele_bot_client/src/app/mod.rs`:
  - 加 `pub struct DeviceCache { pub inputs: Vec<DeviceInfoDto>, pub outputs: Vec<DeviceInfoDto>, pub loaded_at: Instant }` (Default 实现: 空 vec + Instant::now())
  - `App` 加 `pub devices: DeviceCache` 字段
  - `App::default()` 初始化空 cache
- [ ] 4.2 同文件加 `pub enum DeviceKind { Input, Output }` 与 `pub struct DeviceSubmitStamp { pub at: Instant, pub kind: DeviceKind }`. `App` 加 `pub last_device_submit: Option<DeviceSubmitStamp>` 字段
- [ ] 4.3 `crates/ele_bot_client/src/app/route.rs`:
  - 加 `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum SelectingKind { Input, Output }`
  - 加 `#[derive(Debug, Clone)] pub struct SelectingField { pub kind: SelectingKind, pub cursor: usize, pub loading: bool }` 与 `SelectingField::new(kind, len) -> Self`
  - `Route::Settings` 字段加 `selecting: Option<SelectingField>`
  - `Route::from(MenuItem::Settings)` 改造 `selected: 0, editing: None, selecting: None`
- [ ] 4.4 `crates/ele_bot_client/src/app/overlay.rs` (推测路径, 若不存在按项目布局):
  - 加 `Overlay::DevicePicker(SelectingField, Vec<DeviceInfoDto>)`
  - 加 `Overlay::DeviceSwitchFailure { pub kind: DeviceKind, pub old_device_name: String }`
- [ ] 4.5 跑三件套 (此步会破坏 `handle_overlay` 与 ui/viewmodel/settings.rs, 接下来 5 步填回去)

## 5. 客户端输入分发

- [ ] 5.1 `crates/ele_bot_client/src/input/settings.rs`: `SettingsEvent` 加 6 变体 `EnterPicker`, `PickerUp`, `PickerDown`, `PickerConfirm`, `PickerCancel`, `RefreshList`. 每个加 `///` rustdoc
- [ ] 5.2 同文件 `settings::handle` 加 6 个分支:
  - `EnterPicker` 触发 `app.enter_device_picker()` (新增 App 方法, 见 5.6)
  - `PickerUp` / `PickerDown` 改 `selecting.cursor` 含 wrap
  - `PickerConfirm` 提交 + 关 overlay (实现见 5.4)
  - `PickerCancel` 把 `selecting = None`, overlay = None
  - `RefreshList` 发两条 `ListInputDevices` / `ListOutputDevices` 消息
- [ ] 5.3 `crates/ele_bot_client/src/input/mod.rs` `handle_settings`: 进入时按状态判断:
  - `selecting.is_some()` → 把按键翻译为 `Picker*`
  - `editing.is_some()` → 维持现状 (走 overlay 通道)
  - 列表模式 → Enter 翻译为 `SettingsEvent::EnterPicker`, 'r' 翻译为 `SettingsEvent::RefreshList`
- [ ] 5.4 同文件 `handle_overlay` 加 2 个 match 分支:
  - `Overlay::DevicePicker(field, devices)`: Up/Down 改 field.cursor (wrap 含 loading 占位), Enter 提交 (`app.confirm_device_picker()`, 见 5.6), Esc 关, 'r' 切 loading=true + 发两条 List 消息
  - `Overlay::DeviceSwitchFailure { .. }`: Esc 关闭, 同时清 `last_device_submit`
- [ ] 5.5 `handle_settings` 在分支前, 若 `selecting.is_some()` 但 `selecting.loading == true` 且 'r' 键, 直接发两条 List 消息 (避免双跳到 PickerCancel)
- [ ] 5.6 `crates/ele_bot_client/src/app/mod.rs` 加 3 个 App 方法:
  - `pub fn enter_device_picker(&mut self, kind: SelectingKind)` —— 设 `route.selecting`, `overlay`, 若 `self.devices.{inputs,outputs}` 为空则 `loading=true` 同时发两条 List 消息
  - `pub fn picker_up(&mut self)` / `picker_down(&mut self)` —— 改 selecting.cursor
  - `pub fn confirm_device_picker(&mut self)` —— 提交并清状态 (具体写 `config.speech_name` 或 `output_device`, 设 `last_device_submit`, 发 `SetConfig`)
- [ ] 5.7 跑三件套

## 6. 客户端 ws 客户端

- [ ] 6.1 `crates/ele_bot_client/src/ws/client.rs` (或同功能模块): 加 `ServerEvent::InputDevices { devices }` 分支, 写 `app.devices.inputs = devices`, 更新 `loaded_at`, 若 `selecting.kind == Input && selecting.loading` 则清 loading 并把 overlay 列表替换为新 devices
- [ ] 6.2 处理 `OutputDevices` 同上
- [ ] 6.3 加 `ServerEvent::Error { message }` 在 `last_device_submit` 1 秒窗口内的失败 UX:
  - `Instant::now() - last_device_submit.at <= 1s && last_device_submit.kind 一致`
  - 设 `overlay = Some(Overlay::DeviceSwitchFailure { kind, old_device_name })`
  - 清 `last_device_submit` (只一次)
- [ ] 6.4 跑三件套

## 7. 客户端 UI 渲染

- [ ] 7.1 `crates/ele_bot_client/src/ui/viewmodel/settings.rs`:
  - 加 `selecting: Option<SelectingField>`, `device_cache: DeviceCache`, `failure_overlay: Option<Overlay::DeviceSwitchFailure>` 字段 (在 `SettingsViewModel` 中)
  - `from_app` 实现更新, 把以上字段填上
- [ ] 7.2 找到实际渲染 Settings 页的文件 (`ui/pages/settings.rs` 或同等): 加 picker overlay 绘制
  - 标题区: `选择麦克风` / `选择扬声器`
  - 单列: `系统默认` + 设备行
  - driver 部分用 `Style::default().dim()` 或 ratatui 灰 style
  - name 用 `Style::default()` 亮色
  - `(Nch / Hz)` dim 后缀
  - 高亮行加 `> ` 前缀或用反色背景
- [ ] 7.3 加载中分支: 单行 `<加载中...>` 居中, 屏蔽 Enter (实现可通过 overlay.len() == 0 + loading 态组合)
- [ ] 7.4 空列表分支: 单行 `<无可用设备>` 屏蔽 Enter
- [ ] 7.5 Settings 行: 加 "扬声器" 行 (`麦克风` / `扬声器` 两条), 值显示当前 `display` (查 `app.devices` 找名字匹配的 DTO 的 `display`), 找不到则显示原 `config.speech_name` 字符串
- [ ] 7.6 failure transient 渲染: 中央弹窗, 标题 `设备切换失败`, 内容 `已保留 <old_device_name>` + 错误明细
- [ ] 7.7 跑三件套

## 8. 提交路径端到端

- [ ] 8.1 在 `app.confirm_device_picker()` (5.6 中加):
  - `name = if cursor == 0 { "".to_string() } else { devices[cursor - 1].name.clone() };`
  - 按 `kind` 写 `config.speech_name` 或 `config.output_device`
  - `last_device_submit = Some(DeviceSubmitStamp { at: Instant::now(), kind: DeviceKind::from(kind) })`
  - `route.selecting = None`
  - `overlay = None`
  - `tx.send(ClientMessage::SetConfig { config: self.config.clone() })`
- [ ] 8.2 在从 `Nav` 进入 `Settings` 的入口 (查 `handle_nav`, 现有 `Route::from(last)` 处附近), 若 `last == MenuItem::Settings`, 追加两条 `ListInputDevices` / `ListOutputDevices`
- [ ] 8.3 跑三件套

## 9. 失败 UX 端到端

- [ ] 9.1 在 ws 客户端 `ServerEvent::Error` 已有分支 (6.3 完成), 加日志 `device switch failure detected`
- [ ] 9.2 验证: 在 `voice::find_input_device` 故意返回 Err 时 (测试可注入或单元测试), overlay 应该弹出
- [ ] 9.3 Esc 关闭 flow 已在 5.4 完成, 跑一遍手动验证
- [ ] 9.4 跑三件套

## 10. 收尾

- [ ] 10.1 把 `openspec/changes/2026-07-21-enhance-device-picker/specs/audio-device-picker/spec.md` 写好 (Δ 三条 ADDED Requirements)
- [ ] 10.2 `cargo build --release` 一次性跑通 (release 模式会暴露更多 dead code / 优化警告)
- [ ] 10.3 `cargo test --all-features --all-targets` 跑测
- [ ] 10.4 手动验证清单 (在修复真机跑):
  - 进设置页 → 收到 `*Devices` 两条
  - 切麦克风 → 后端 log 显示 `rebuild_voice` + 旧 VoiceManager Drop
  - 故意把新设备名改成非法字符串 (测 failure): 弹 transient overlay, 配置未变更
- [ ] 10.5 `cargo fmt --all && cargo clippy --all-features --all-targets -- -D warnings && cargo check --all-features --all-targets` 最终一次
- [ ] 10.6 三件套过则提交. commit 信息首行: `[新增] 设备选择器(补回 + driver 渲染 + 失败 transient)`
- [ ] 10.7 跑 `openspec sync-specs 2026-07-21-enhance-device-picker` 把 spec Δ 合入主 spec
- [ ] 10.8 跑 `openspec archive 2026-07-21-enhance-device-picker` 归档, 检查 `openspec/specs/audio-device-picker/spec.md` 已经合并
- [ ] 10.9 跑 `cargo build --release` 再确认 release OK
- [ ] 10.10 推送 PR

## 风险与回退

- 任何一步跑三件套失败, **不进下一步**, 直接修该步
- 若 picker UI 出现 ratatui 渲染异常 (e.g. 终端不支持 Unicode), 在 7.2 加 fallback 用 ASCII
- 若 transient overlay 5 秒计时器无合适位置实现, 可改用"Esc 必关 + 5 秒可选"; 但必先要 Esc 路径
