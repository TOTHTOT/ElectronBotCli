# audio-device-picker Specification

## Purpose
TBD - created by archiving change audio-device-picker. Update Purpose after archive.
## Requirements
### Requirement: 协议支持枚举音频设备
系统 SHALL 提供让客户端拉取当前可用音频输入/输出设备列表的协议消息.

`ClientMessage` SHALL 至少新增以下两个变体:
- `ListInputDevices` — 请求输入设备列表
- `ListOutputDevices` — 请求输出设备列表

`ServerEvent` SHALL 至少新增以下两个变体, 每个携带一个 `Vec<DeviceInfoDto>`:
- `InputDevices { devices }` — 输入设备列表响应
- `OutputDevices { devices }` — 输出设备列表响应

`DeviceInfoDto` SHALL 至少包含:
- `name: String` — 设备真实名称, 用于与服务端 exact match
- `display: String` — UI 显示用, 已拼接 driver / channels / sample_rate
- `driver: Option<String>` — 驱动名 (Windows 下如 `WASAPI`, Linux 下如 `ALSA`), 用于客户端展示区分

#### Scenario: 客户端请求输入设备列表
- **WHEN** 客户端发送 `ClientMessage::ListInputDevices`
- **THEN** 服务端 SHALL 在 `ServerEvent::InputDevices` 中返回当前所有可用输入设备的 `DeviceInfoDto` 列表
- **AND** 列表内容 SHALL 由 `voice::list_input_devices()` 提供 (cpal 实时枚举)

#### Scenario: 客户端请求输出设备列表
- **WHEN** 客户端发送 `ClientMessage::ListOutputDevices`
- **THEN** 服务端 SHALL 在 `ServerEvent::OutputDevices` 中返回当前所有可用输出设备的 `DeviceInfoDto` 列表
- **AND** 列表内容 SHALL 由 `voice::list_output_devices()` 提供

### Requirement: 设置页提供设备选择 UI
设置页 SHALL 在"麦克风"和"扬声器"两行用设备列表选择器替换当前的文本编辑模式.

每个设备选择行 SHALL 支持以下交互:
- 进入"选择"模式后, 在客户端本地构造的第一项 SHALL 为"系统默认" (空 name, display = `<系统默认>`)
- 后续项 SHALL 为服务端返回的设备, 顺序与 `DeviceInfoDto` 数组一致
- 用户按 ↑/↓ SHALL 在项之间移动高亮; 按 Enter SHALL 提交当前高亮项; 按 Esc SHALL 取消并返回设置列表

提交后, 系统 SHALL:
- 更新 `app.config.speech_name` (输入) 或 `app.config.output_device` (输出) 为所选项的 `name` (空字符串表示系统默认)
- 通过 `ClientMessage::SetConfig` 发送给服务端
- 触发服务端的 `VoiceManager` 热重建 (见热重建需求)

#### Scenario: 用户选择非默认麦克风
- **WHEN** 设置页麦克风行进入选择模式, 用户高亮第 2 项 (某真实设备), 按 Enter
- **THEN** 客户端 SHALL 将 `app.config.speech_name` 设为该设备的 `name`
- **AND** 客户端 SHALL 通过 `SetConfig` 发送更新后的 `AppConfig` 给服务端
- **AND** 服务端 SHALL 重建 `VoiceManager`, 使 ASR 流使用新设备

#### Scenario: 用户选择"系统默认"
- **WHEN** 用户高亮第 1 项 (`<系统默认>`), 按 Enter
- **THEN** 客户端 SHALL 将 `app.config.speech_name` 设为 `""` (空字符串)
- **AND** 服务端 SHALL 在重建 `VoiceManager` 时回退到 `cpal::default_input_device()` / `cpal::default_output_device()`

#### Scenario: 用户取消选择
- **WHEN** 用户在选择模式按 Esc
- **THEN** 系统 SHALL 不修改 `app.config.speech_name` / `app.config.output_device`
- **AND** 系统 SHALL 返回设置列表, 高亮回到原来的行

### Requirement: 进入设置页时拉取设备列表
客户端 SHALL 在进入 `Route::Settings` 时自动发送 `ListInputDevices` 和 `ListOutputDevices`, 不需要用户额外操作.

进入路径包括:
- 从 `Route::Nav` 进入 (按 Enter 选中"设置"菜单项)
- 应用启动时若初始路由是 `Route::Settings` (后续扩展)

#### Scenario: 从导航进入设置页
- **WHEN** 用户在 Nav 页面选中"设置"并按 Enter
- **THEN** 客户端 SHALL 立即发送 `ClientMessage::ListInputDevices` 和 `ClientMessage::ListOutputDevices`
- **AND** 在收到响应前, 选择模式 SHALL 显示 `<加载中...>`, 屏蔽 Enter

### Requirement: 设置页支持手动刷新设备列表
设置页 SHALL 在列表模式 (`Route::Settings { selecting: None, editing: None }`) 下响应 `[R]` 键, 重新发送 `ListInputDevices` 和 `ListOutputDevices`.

#### Scenario: 用户按 R 刷新
- **WHEN** 用户在设置列表模式按 `R` 键
- **THEN** 客户端 SHALL 重新发送 `ListInputDevices` 和 `ListOutputDevices`
- **AND** 当前选择项 SHALL 保留 (若仍在新列表中); 若不在, SHALL 退回到第 1 项 (`<系统默认>`)

### Requirement: 服务端支持 VoiceManager 热重建
服务端 SHALL 在 `AppConfig.speech_name` 或 `AppConfig.output_device` 发生变化时, 重建 `VoiceManager`.

`SharedState` SHALL 暴露 `pub fn rebuild_voice(&self) -> anyhow::Result<()>`. 该函数 SHALL:
- 用当前 `AppConfig` 构造新的 `VoiceManager`
- 替换 `self.voice` 中的 `Option<Arc<VoiceManager>>`
- 失败时 SHALL 保持旧 `VoiceManager` 不变并通过 `ServerEvent::Error` 通知客户端

旧 `VoiceManager` SHALL 通过 `Drop` 自然释放; 系统 MUST NOT 强制 join 已分离的识别线程.

#### Scenario: 输入设备变化触发重建
- **WHEN** 客户端发来 `SetConfig`, `config.speech_name` 与当前 `VoiceManager` 的输入设备不同
- **THEN** 服务端 SHALL 调用 `rebuild_voice()` 重建 `VoiceManager`
- **AND** 新 `VoiceManager` SHALL 使用新设备运行 ASR 流
- **AND** 旧 `VoiceManager` 的 cpal stream SHALL 在 Drop 时停止

#### Scenario: 重建失败保持旧实例
- **WHEN** `rebuild_voice()` 内部 `init_voice` 返回 Err (例如新设备被独占占用)
- **THEN** 服务端 SHALL 保留原 `VoiceManager` 不变
- **AND** 服务端 SHALL 广播 `ServerEvent::Error { message }` 给所有客户端

### Requirement: 设备列表为空时的兜底显示
当服务端返回空设备列表 (Vec 长度为 0), 客户端 SHALL:
- 在选择模式显示 `<无可用设备>` 占位行
- 屏蔽 Enter (无法选中不存在的项)
- 仍允许 Esc 返回

#### Scenario: 无可用输入设备
- **WHEN** 客户端收到 `ServerEvent::InputDevices { devices: [] }`, 用户进入麦克风选择模式
- **THEN** UI SHALL 显示 `<无可用设备>`
- **AND** UI SHALL 不响应 Enter
- **AND** UI SHALL 响应 Esc 返回设置列表

