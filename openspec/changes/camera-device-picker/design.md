## Context

服务端已经在共享 `EventBus` 同时驱动 LLM 流和帧流 (`media/video/types.rs::FrameCache` 即是 `EventBus` 的 type alias)。`SharedState::new` 在启动时构造单个 `VideoCapture` 永远持有;protocol 层也没有任何枚举/选择摄像头的消息,所以 TUI 用户想要切换 USB 摄像头就必须停服务改 `config.toml`。

音频端用 `rebuild_voice()` 完成过"运行时软替换":旧实例 `take()` 移出 → `running=false` 通知旧 ASR 线程退出 → sleep 60ms 让旧 cpal 让出 → `init_voice` 构造新实例 → 替换 + Drop。**摄像头侧缺同一套机制**,但 `VideoCapture` 的 `Drop` 实现已经会 `handle.join()`,所以"自动等线程退出"那一步是免费的,我们只需要把它包装成 `rebuild_video()`。

客户端 picker 三件套已经成型:
- `Overlay::DevicePicker { selecting, devices: Vec<DeviceInfoDto> }`
- `App::send_device_list_request(SelectingKind)` + `refresh_picker_after_load`
- `ui/viewmodel/settings.rs::SettingsViewModel::from_app` 把 `DeviceInfoDto` 转成 `PickerRow`

这套全是为音频写的;摄像头要走形状非常相似的路径。

## Goals / Non-Goals

**Goals:**

- 协议层暴露摄像头枚举(picker 输入)
- TUI settings 列表新增"摄像头"行,点击进 picker,提交改 `config.camera_index`
- 服务端检测 `camera_index` 变化就热切换,不必重启 ws
- 热切换失败的回退语义与 `rebuild_voice` 完全一致:config 已持久化,推 `ServerEvent::Error`,旧实例继续工作
- 复用现有 `EventBus` 作为帧通道,**不引入第二条帧流**

**Non-Goals:**

- 不做"边预览边换"(picker 期间禁掉 ws 期间帧,提交后才生效)
- 不做摄像头热插拔检测(只支持 picker 主动选)
- 不暴露摄像头其他属性(帧率/曝光等)
- 不动 `AppConfig` 字段形状(沿用 `camera_index: String`)

## Decisions

### 决策 1: `Overlay::DevicePicker.devices` 改成 `Vec<PickerEntry>`

**备选:** 留 `Vec<DeviceInfoDto>`,加 `Option<CameraInfoDto>` 字段(变体少,只多一项)。

**选定:** `enum PickerEntry { Audio(DeviceInfoDto), Camera(CameraInfoDto) }`。

**原因:** `ui/viewmodel/settings.rs::from_app` 第 116-122 行的 `for d in devices { split_driver_and_suffix(d) ... }` 用的是 `DeviceInfoDto` 的字段(`display` + `driver`)。`CameraInfoDto` 没有 driver 后缀可拆,渲染层必须按类型分派;**统一 enum 让 `from_app` 一处 match 决定走哪份渲染逻辑**,避免嵌套 if-else。

### 决策 2: 协议层不加 `SelectCamera`

**备选:** 加 `ClientMessage::SelectCamera { id }`,picker 选中后单发一条。

**选定:** 不加,只走 `SetConfig { config }`。

**原因:** picker 完成时整个 `AppConfig` 已经更新完毕,客户端已经持有新的 `camera_index`。`set_config` 检测字段变化后 `rebuild_video`,与音频端同款链路。**协议面多一个消息就多一处文档测试,且 picker 的"提交"反馈链路(`ServerEvent::Config`)已经够用。**

### 决策 3: `state.video` 改为 `Mutex<Option<Arc<VideoCapture>>>`

**备选:** 维持 `Mutex<VideoCapture>`,在 `rebuild_video` 里用 `std::mem::replace` 拿出 + drop。

**选定:** `Mutex<Option<Arc<VideoCapture>>>`。

**原因:**
1. 跟 `state.voice` 现有形状一致(`Mutex<Option<Arc<VoiceManager>>>`),后端代码语义对称
2. `take()` + Drop 的所有权转移是显式的,`mem::replace` 需要 placeholder
3. `Option` 让"启动失败,video 不可用"的状态合法表达

### 决策 4: 热切换靠 `VideoCapture::Drop` 里的 `handle.join()`

**备选:** 像 `rebuild_voice` 那样手动 `running=false` + `sleep(60ms)` 等线程退出。

**选定:** 直接 `take()` 旧 Arc → drop(`VideoCapture::Drop` 接管)。

**原因:** `crates/ele_bot_server/src/media/video/capture.rs:69-78` 的 `Drop` 已经把 `running.store(false)` + `handle.join()` 做完了。**复用 Drop 比手动写一遍可靠**(capture thread 在 `capture_frames` 内 `nokhwa::Camera::frame()` 调用,阻塞会被取消)。**代价:** Drop 时会等当前一帧抓完(几十毫秒到 100ms),用户感知是 picker 提交后短暂黑屏 1-2 帧,可接受。

### 决策 5: 失败回退 `pub fn rebuild_video` 返回 `Result<()>`, `set_config` 镜像音频端语义

```rust,ignore
if let Err(e) = self.rebuild_video() {
    log::warn!("rebuild_video failed: {e:?}");
    self.bus_tx.publish(BusEvent::ServerEvent(ServerEvent::Error {
        message: format!("camera rebuild failed: {e}"),
    }));
    // config 已经 cfg.save() + write, 旧 Option<Arc<VideoCapture>> 已被 take()
    // 这里重建一个新实例用回旧 index, 让视频流不至于断
    if let Err(e2) = self.rebuild_video_fallback() {
        log::error!("camera rebuild fallback failed: {e2:?}");
    }
}
```

为了避免"切失败 → 没视频",在错误分支里**拿回旧 index 重建一次**,与音频端"重建失败时旧 VoiceManager 仍在"不同(摄像头没法像 VoiceManager 那样 `take()` 后局部 hold)。**这里摄像头端的取舍是:错误也保证有视频流,代价是 config 已持久化为用户意图但显示流跑的是旧 index;客户端靠 `ServerEvent::Error` 提示用户"切换未生效"。**

## Risks / Trade-offs

| 风险 | 缓解 |
|---|---|
| `nokhwa::Camera` 在新线程重建时被旧设备句柄占用导致打开失败 | 旧的 `Drop` join 已让出 USB 句柄;若仍失败走决策 5 fallback |
| `BusEvent` 容量 1024 满时 webcam 帧被丢弃,影响 preview / face tracking | 与现状一致;不在本次 change 范围 |
| nokhwa `CameraInfo` 字段在 Windows / macOS / Linux 不一致,`display` 拼出来的字符串差异大 | `display` 是给人类看的字符串,匹配是按 `id` (nokhwa index),不依赖字符串相等 |
| `CameraIndex::Index(0)` 当默认值但 USB 摄像头序号可能跳号 | 已有逻辑(`SharedState::new` 第 79-84 行),不在本次改动 |
| `From<SelectingKind> for DeviceKind` 加 `Camera` 变体后语义不清(Camera 没有 `DeviceKind`) | `DeviceKind` 仅在 `Overlay::DeviceSwitchFailure` 用,Camera 失败走单独路径或不映射 — 实现时显式 `unimplemented!` 并在 tasks.md 里标"待你确认是否需要" |

## Migration Plan

1. 协议层先落地:加 `ListCameras` / `Cameras` / `CameraInfoDto`,不影响老客户端(`from_json` 容错)
2. 服务端加 `list_cameras_dto()` + `parse_camera_index()`,单元测试覆盖 `nokhwa CameraInfo -> DTO`
3. `state.video` 改 Option<Arc> + `rebuild_video` 落地
4. ws.rs `handle_command` 加 `ListCameras` 分支
5. 客户端:SelectingKind → Camera → PickerEntry → DeviceCache.cameras → from_app / apply_event 路径
6. TUI 渲染层加 "摄像头" SettingItem 行
7. 三连: `cargo fmt --all && cargo clippy --all-features --all-targets -- -D warnings && cargo check --all-features --all-targets`
8. 手动:启动服务 → 多摄像头机型下 picker 选 → 确认帧流切到新设备 / 失败时看到 Error overlay

回滚:协议新增字段不破坏向后兼容;`state.video` 形状改动可以通过仅回滚 `state.rs / ws.rs / capture.rs` 回到直接 `VideoCapture`。
