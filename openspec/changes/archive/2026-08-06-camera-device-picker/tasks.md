## 1. 协议层 (proto)

- [x] 1.1 在 `crates/ele_bot_proto/src/types.rs` 新增 `CameraInfoDto { id, name, display }`,带 `///` rustdoc(职责/边界/Examples),`#[derive] Debug/Clone/Serialize/Deserialize` 与现有 `DeviceInfoDto` 对齐
- [x] 1.2 在 `crates/ele_bot_proto/src/messages.rs::ClientMessage` 加 `ListCameras` 变体(snake_case 自动 rename),保持 `ListInputDevices/ListOutputDevices` 同款不带字段
- [x] 1.3 在 `crates/ele_bot_proto/src/messages.rs::ServerEvent` 加 `Cameras { cameras: Vec<CameraInfoDto> }`,并补 roundtrip 单元测试(协议层 `crate::messages::tests` 已有 `list_devices_request_roundtrip` 风格,沿用同一组断言)

## 2. 服务端 capture + DTO

- [x] 2.1 在 `crates/ele_bot_server/src/media/video/capture.rs` 新增 `pub fn list_cameras_dto() -> Vec<CameraInfoDto>`,内部调 `VideoCapture::list_cameras()`,把每个 `nokhwa::CameraInfo` 转成 DTO,补 rustdoc + 单测(`Option<Vec<CameraInfoDto>>` 不变量: 设备名为空时降级到 `format!("Camera {index}")`)
- [x] 2.2 新增 `pub(crate) fn parse_camera_index(s: &str) -> CameraIndex` —— 与 `SharedState::new` 第 79-84 行同等语义,让 `rebuild_video` 与启动路径共用同一函数(避免重复实现)

## 3. 服务端 state + 热切换

- [x] 3.1 `crates/ele_bot_server/src/state.rs::SharedState` 字段 `video: Mutex<VideoCapture>` → `video: Mutex<Option<Arc<VideoCapture>>>`,改 `Self::new()` 构造路径(`Option<Arc<VideoCapture>>::Some(Arc::new(video_capture))`)
- [x] 3.2 新增 `pub fn current_video_config(&self) -> String`,镜像 `current_audio_config`,仅读 `self.config.read().unwrap().camera_index.clone()`
- [x] 3.3 新增 `pub fn rebuild_video(&self) -> anyhow::Result<()>`,按 design §决策 5 顺序 take → Drop → new(用 `CameraIndex::Index(parse_camera_index(...))`)→ `start_capture_frames_thread` → 写回 Option。带 rustdoc + `Examples`
- [x] 3.4 修改 `pub fn set_config(&self, cfg) -> anyhow::Result<()>` 增加 `video_changed = cfg.camera_index != current_video_config()` 分支;`rebuild_video()` 失败时按 decision 5 走 fallback(用旧 `camera_index` 再 rebuild 一次,确保视频流不断),错误照推 `ServerEvent::Error`
- [x] 3.5 新增 `pub fn video(&self) -> Option<Arc<VideoCapture>>` 访问器,用于 `WebPreview` / `face tracking` 等需要在运行时拿当前实例的旧调用点(本任务先不动这些调用点,先提供接口;后续 patch 沿用)

## 4. 服务端 ws 处理

- [x] 4.1 `crates/ele_bot_server/src/ws.rs::handle_command` 新增 `ClientMessage::ListCameras` 分支:`let devices = crate::media::video::capture::list_cameras_dto(); out_tx.send(ServerEvent::Cameras { cameras: devices })?;`

## 5. 客户端类型 / picker

- [x] 5.1 `crates/ele_bot_client/src/app/route.rs::SelectingKind` 加 `Camera` 变体,补 doc
- [x] 5.2 `crates/ele_bot_client/src/app/overlay.rs` 新增 `pub enum PickerEntry { Audio(DeviceInfoDto), Camera(CameraInfoDto) }`,`Overlay::DevicePicker.devices` 类型从 `Vec<DeviceInfoDto>` 改为 `Vec<PickerEntry>`;`From<SelectingKind> for DeviceKind` 加 `Camera` 分支(若 Camera 失败无 `DeviceKind` 则 `unimplemented!()` 并在 PR review 时讨论是否需要,代码注释标注)
- [x] 5.3 `crates/ele_bot_client/src/app/mod.rs::App` 字段 `devices` 加 `cameras: Vec<CameraInfoDto>`,初始化为 `Vec::new()`
- [x] 5.4 `App::send_device_list_request(SelectingKind)` 加 `Camera => ClientMessage::ListCameras`
- [x] 5.5 `App::refresh_picker_after_load(SelectingKind)` 加 `Camera` 分支:从 `app.devices.cameras` 取数据,转 `PickerEntry::Camera`
- [x] 5.6 `App::apply_event` 处理 `ServerEvent::Cameras { cameras }`:写进 `app.devices.cameras`,然后调 `refresh_picker_after_load(SelectingKind::Camera)`

## 6. 客户端 UI

- [x] 6.1 `crates/ele_bot_client/src/ui/viewmodel/settings.rs::SettingsViewModel::from_app` 在现有 `SettingItem` 列表末端追加 "摄像头" 行:`label = "摄像头"`,`value = display_for_camera(&app.devices.cameras, &app.config.camera_index)`,新增辅助函数 `display_for_camera(devices: &[CameraInfoDto], id: &str) -> String`(空 id → `<系统默认>`,否则按 id 找 device 拿 display,找不到回退 id 本身)
- [x] 6.2 同文件处理 `Overlay::DevicePicker` 的 `PickerEntry::Camera` 渲染:在 `from_app` 第 116-122 行的 `for entry in devices` 循环里 match 两种 entry,`Camera(d)` 直接把 `d.display` 当 `label`、`dim_suffix` 留空(摄像头没有 driver/通道后缀);`Audio(d)` 沿用现有 `split_driver_and_suffix`
- [x] 6.3 `crates/ele_bot_client/src/ui/pages/settings.rs` 渲染新行(若该文件按 label 分派则添加 "摄像头" 入口;若按 `Vec<SettingItem>` 平铺渲染则无需改)。**先确认文件内容再动**

## 7. 三连与验证

- [x] 7.1 `cargo fmt --all`(任务 1-6 完成后)
- [x] 7.2 `cargo clippy --all-features --all-targets -- -D warnings`(任务 1-6 完成后)
- [x] 7.3 `cargo check --all-features --all-targets`(任务 1-6 完成后)
- [x] 7.4 `openspec validate camera-device-picker --strict`,确保全部 artifact 通过 schema 校验
- [x] 7.5 协议层单测 `cargo test -p ele_bot_proto list_cameras -- --nocapture`,验证 `ListCameras` / `Cameras{cameras}` roundtrip
