## Why

ElectronBotCli 已经把"麦克风/扬声器"的设备选择做成 picker 弹窗(参考 `crates/ele_bot_server/src/media/voice/mod.rs::list_*_devices` + 协议 `ListInputDevices/ListOutputDevices` + `crates/ele_bot_client/src/app/overlay.rs::Overlay::DevicePicker`)。但**摄像头没有任何 UI 入口**:`AppConfig.camera_index` 只在 `SharedState::new()` 解析后写死,settings 列表里也没有这一项,导致 USB 摄像头换位置 / 多摄像头机型的用户必须在 `config.toml` 手改 `camera_index` 才能切换。

## What Changes

- **协议层** (`crates/ele_bot_proto`): 加 `ClientMessage::ListCameras` 与 `ServerEvent::Cameras { cameras: Vec<CameraInfoDto> }`,新 DTO `CameraInfoDto { id, name, display }` 镜像音频端 `DeviceInfoDto` 的形状。`AppConfig.camera_index: String` 不改。
- **服务端** (`crates/ele_bot_server`): `media/video/capture.rs` 加 `list_cameras_dto()` 把 `nokhwa::CameraInfo` 转成 DTO;`ws.rs::handle_command` 加 `ListCameras` 分支。`state.rs` 把 `video: Mutex<VideoCapture>` 改为 `Mutex<Option<Arc<VideoCapture>>>`,新增 `rebuild_video()` / `current_video_config()`;`set_config` 检测到 `camera_index` 变化时热切。
- **客户端** (`crates/ele_bot_client`): `route.rs::SelectingKind` 加 `Camera`;`overlay.rs::Overlay::DevicePicker.devices` 改为 `Vec<PickerEntry>`,新增 `enum PickerEntry { Audio(DeviceInfoDto), Camera(CameraInfoDto) }`;`app/mod.rs` 增加 `devices.cameras` 缓存、`send_device_list_request` / `refresh_picker_after_load` 加 `Camera` 分支,新增 `apply_event(ServerEvent::Cameras)` 路径;`ui/viewmodel/settings.rs` 在 8 行 `SettingItem` 列表中新增 "摄像头" 行。
- **热切换语义**: 与 `rebuild_voice` 对齐 — 旧实例先 `take()` 移出,Drop 时 `VideoCapture` 自带的 `Drop` join 自动等 capture 线程退出;新实例构造后替换。失败时 config 仍持久化,通过 `ServerEvent::Error` 告知客户端。

## Capabilities

### New Capabilities

- `camera-device-picker`: 协议层的摄像头枚举 / picker 选择 / TUI 渲染 / 热切换语义 / 失败回退。

### Modified Capabilities

- 无 — `AppConfig.camera_index` 字段已存在;`selecting-audio-device` 等其他 spec 不受影响。

## Impact

| Crate | 文件 | 改动 |
|---|---|---|
| `ele_bot_proto` | `src/messages.rs` | 加 `ClientMessage::ListCameras` / `ServerEvent::Cameras` |
| `ele_bot_proto` | `src/types.rs` | 加 `CameraInfoDto` |
| `ele_bot_server` | `src/media/video/capture.rs` | 加 `list_cameras_dto()` / `parse_camera_index()` |
| `ele_bot_server` | `src/state.rs` | `video` 改 `Mutex<Option<Arc<VideoCapture>>>`;加 `rebuild_video` / `current_video_config` / `video()` 访问;`set_config` 加 video 分支 |
| `ele_bot_server` | `src/ws.rs` | `handle_command` 加 `ListCameras` 分支 |
| `ele_bot_client` | `src/app/route.rs` | `SelectingKind` 加 `Camera` |
| `ele_bot_client` | `src/app/overlay.rs` | `Overlay::DevicePicker.devices: Vec<PickerEntry>` + `enum PickerEntry` |
| `ele_bot_client` | `src/app/mod.rs` | `DeviceCache.cameras` + `send_device_list_request` / `refresh_picker_after_load` / 处理 `ServerEvent::Cameras` |
| `ele_bot_client` | `src/ui/viewmodel/settings.rs` | `SettingItem` 列表新增 "摄像头" 行,`from_app` 处理 `PickerEntry::Camera` 分支 |

无新增依赖,无协议版本号变更(沿用现有 snake_case JSON tag 模式)。
