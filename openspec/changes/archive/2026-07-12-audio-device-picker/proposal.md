## Why

设置页麦克风目前让用户手敲设备名做服务端 exact name match, 用户体验差且易错 (虚拟设备名长, 同名设备无法区分). 服务端 `list_input_devices` / `list_output_devices` 已实现并带有 driver / channels / sample_rate, 但 TUI 从未调用. 同时 `output_device` 字段已在 `AppConfig` 里存在, 设置页却没暴露, 扬声器选择能力空白.

## What Changes

- 设置页把"麦克风名称"从文本编辑改成设备列表选择; 新增"扬声器"行做同样改造
- 设备选项展示 `DeviceInfo.display` (即 `name (driver, Nch, NHz)`), 第一项为"系统默认" (空 name)
- 客户端新增 `Route::Settings` 的子模式进入设备选择 (A 方案: 子路由, 与 `DeviceControl::Idle` 风格一致)
- 选择提交后**立即重建 VoiceManager** (热生效), 同时写回 `AppConfig` 并持久化到 `config.toml`
- 新增协议消息 `ListInputDevices` / `ListOutputDevices` 与 `InputDevices` / `OutputDevices` 回包, 设备信息通过 `DeviceInfoDto` 暴露 name + display + driver
- 进入 Settings 路由时一次性拉取输入/输出设备列表; 设置页内 `[R]` 键可手动刷新

## 非目标 (Non-goals)

- 不存 driver 到 `AppConfig` (维持只存 name 的最小改动, UI 用 display 区分)
- 不改 ASR / TTS 模型加载路径; 只在保持 `VoiceManager::new()` 签名不变的前提下支持重建
- 不引入 fuzzy / 子串匹配; 选择即选定, 匹配逻辑沿用现有 exact match

## Capabilities

### New Capabilities

- `audio-device-picker`: 设置页输入/输出设备选择器 — 协议、设备列表拉取、UI 子路由、热重建 VoiceManager

### Modified Capabilities

无 (项目尚未有 capability spec, 这是第一个)

## Impact

- `crates/ele_bot_proto/src/messages.rs`: 新增 4 个枚举变体, 新增 `DeviceInfoDto`
- `crates/ele_bot_proto/src/types.rs`: `AppConfig` 不动
- `crates/ele_bot_server/src/ws.rs`: 处理新消息, 复用 `voice::list_input_devices` / `list_output_devices`
- `crates/ele_bot_server/src/state.rs`: 新增 `rebuild_voice(&AppConfig)` 公开方法
- `crates/ele_bot_server/src/media/voice/mod.rs`: 可能抽出构造/析构辅助函数以便热重建
- `crates/ele_bot_client/src/app/route.rs`: `Route::Settings` 增加 selecting 子模式 (或新增 `Route::DevicePicker`)
- `crates/ele_bot_client/src/ui/viewmodel/settings.rs`: 缓存 input/output 设备列表与当前选中 index
- `crates/ele_bot_client/src/ui/pages/settings.rs`: 麦克风/扬声器行渲染改为显示选中设备 display
- `crates/ele_bot_client/src/ui/pages/`: 新增设备选择子页 (或复用 settings 页同区域)
- `crates/ele_bot_client/src/input/settings.rs` + `mod.rs`: 增加 picker 内的 Up/Down/Enter/Esc/R 映射