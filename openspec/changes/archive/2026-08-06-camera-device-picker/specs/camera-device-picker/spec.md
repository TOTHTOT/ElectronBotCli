## ADDED Requirements

### Requirement: 协议层暴露摄像头枚举

协议层 MUST 提供一对消息让客户端请求并接收摄像头列表:

- `ClientMessage::ListCameras` — 客户端触发枚举
- `ServerEvent::Cameras { cameras: Vec<CameraInfoDto> }` — 服务端响应,`cameras` 为空时表示当前没有可用摄像头
- DTO `CameraInfoDto { id: String, name: String, display: String }` 必须镜像音频端 `DeviceInfoDto` 形状,字段语义:
  - `id` — nokhwa `CameraInfo.index` 序列化,与 `AppConfig.camera_index` 一一对应
  - `name` — nokhwa `CameraInfo.human_readable_name`(或 `description`),作为旧 config 兜底匹配
  - `display` — 给人类看的拼接串,e.g. `"Integrated Camera (id=0, USB)"`

`AppConfig.camera_index: String` 字段不改动;`camera_index == ""` 仍走 `nokhwa::utils::CameraIndex::Index(0)` 默认,与现状一致。

#### Scenario: 客户端请求枚举

- **WHEN** 服务端 ws 收到 `ClientMessage::ListCameras`
- **THEN** 服务端调用 `VideoCapture::list_cameras()`(或等价 API)查询 nokhwa
- **AND** 把每个 `nokhwa::CameraInfo` 映射成 `CameraInfoDto`
- **AND** 通过 `ServerEvent::Cameras { cameras: Vec<CameraInfoDto> }` 回送

#### Scenario: 无可用摄像头

- **WHEN** 系统没有 USB 摄像头或被独占占用
- **THEN** 服务端仍回送 `ServerEvent::Cameras { cameras: vec![] }`(非 Error)
- **AND** 客户端 UI 在 picker 列表只渲染 `<无摄像头>`,不报错

### Requirement: 客户端 picker 支持摄像头选择

TUI 的 `Overlay::DevicePicker` MUST 支持音频和摄像头两类设备,共用同一 picker overlay。

- `SelectingKind` 枚举加 `Camera` 变体
- `Overlay::DevicePicker.devices` 类型从 `Vec<DeviceInfoDto>` 改为 `Vec<PickerEntry>`
- 新枚举 `PickerEntry { Audio(DeviceInfoDto), Camera(CameraInfoDto) }` 提供单一序列供 `SettingsViewModel::from_app` 渲染
- `App::send_device_list_request(SelectingKind)` 根据 `kind` 派发:
  - `Input` → `ListInputDevices`
  - `Output` → `ListOutputDevices`
  - `Camera` → `ListCameras`
- `App::refresh_picker_after_load(SelectingKind)` 同结构派发,在收到对应事件后替换 `Overlay::DevicePicker.devices` 并清 `loading`
- 收到 `ServerEvent::Cameras { cameras }` 时:
  - 若当前 Route 处于 `Settings::selecting == Some(Camera)`,把列表写进 `Overlay::DevicePicker.devices`(统一 `PickerEntry::Camera`),清 `loading`
  - 否则仅把 `app.devices.cameras: Vec<CameraInfoDto>` 缓存更新

#### Scenario: 提交选择

- **WHEN** 用户在 picker 选中第 `i` 行(`i=0` 表示系统默认,`i>0` 对应 `CameraInfoDto`)
- **THEN** 客户端把 `AppConfig.camera_index` 写为 `devices[i-1].id`(默认行为: 设回 `""`)
- **AND** 通过 `ClientMessage::SetConfig { config }` 发回服务端
- **AND** 关闭 picker overlay(`Overlay` 回到 `None`)

#### Scenario: 跨 kind picker 切换

- **WHEN** 用户在 `SelectingKind::Audio` 模式下收到 `Cameras` 事件(异步到达)
- **THEN** 服务端消息必须被忽略(只更新 `app.devices.cameras` 缓存,不进入 picker)

### Requirement: 设置页新增"摄像头"行

`SettingsViewModel::from_app` MUST 在 `SettingItem` 列表中追加一行:

- `label` = `"摄像头"`
- `value` = 当前 `AppConfig.camera_index` 在最近 `ServerEvent::Cameras` 列表中找到的设备 `display`;找不到时回退到 `camera_index` 原始字符串,空值显示 `<系统默认>`

#### Scenario: 渲染当前选择

- **WHEN** 用户进入 Settings 页面,`camera_index` 已有值且与某台摄像头 `id` 匹配
- **THEN** 该行 `value` 显示对应设备 `display`(e.g. `"Integrated Camera (id=0, USB)"`)

#### Scenario: 未配置

- **WHEN** `camera_index == ""`
- **THEN** 该行 `value` 显示 `"<系统默认>"`

### Requirement: 热切换摄像头

服务端 MUST 在 `camera_index` 变化时重建 `VideoCapture`,不重启 ws 服务。

- `SharedState::video` 类型由 `Mutex<VideoCapture>` 改为 `Mutex<Option<Arc<VideoCapture>>>`
- 新增 `SharedState::rebuild_video() -> anyhow::Result<()>`:旧实例 `take()` 移出 → Drop(由 `VideoCapture::Drop` 自动 join capture thread) → 用新 `CameraIndex` 构造新实例 → 启动 capture frames → 写回 `Option`
- `SharedState::set_config(cfg)` 检测到 `cfg.camera_index` 与 `current_video_config()` 不同时调用 `rebuild_video`;成功则推 `ServerEvent::CameraResolution { width, height }`(沿用现有消息)给客户端,失败时推 `ServerEvent::Error { message }`
- `SharedState::current_video_config() -> String` 提供与 `current_audio_config` 对称的访问器

#### Scenario: 正常切换

- **WHEN** 用户提交新 `camera_index`,经 `SetConfig` 送达服务端
- **THEN** 旧 `VideoCapture` 在帧抓完当前帧后被 `Drop` 释放(join capture thread)
- **AND** 新 `VideoCapture` 启动 capture frame 线程,继续向 `EventBus` 推 `ServerEvent::VideoFrame`(或现有变体)
- **AND** 全程不需要重启 ws,不需要重启整个服务端

#### Scenario: 新设备打开失败

- **WHEN** `VideoCapture::new(cam_index, ...)` 报错(USB 占用 / 权限 / 设备消失)
- **THEN** `rebuild_video` 返回 `Err`
- **AND** `set_config` 已先持久化新 config
- **AND** 推 `ServerEvent::Error { message: format!("camera rebuild failed: {e}") }` 给客户端
- **AND** 服务端用回旧 `camera_index` 重建 fallback `VideoCapture`,确保视频流不断(运行时仍能跑)

#### Scenario: 重连场景

- **WHEN** 客户端 ws 断开重连
- **THEN** 服务端 `Config { config }` 推送新连接上的客户端,客户端从 `app.config.camera_index` 恢复"摄像头"行显示
