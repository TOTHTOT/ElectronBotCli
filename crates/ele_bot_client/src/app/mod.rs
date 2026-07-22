//! App 主结构 - 客户端版本
//!
//! 持有 UI 状态 + 网络客户端, 通过 WS 与服务端通信。
//! 不再直接持有任何硬件资源。

pub mod menu;
pub mod mode;
pub mod overlay;
pub mod route;

use crate::net::Client;
use crate::ui::pages::llm_test::LlmTestState;
use crate::ui::pages::tts_test::TtsTestState;
use ele_bot_proto::{
    Action, AppConfig, ClientMessage, DeviceInfoDto, DisplayMode, LlmResponse, Mood, ServerEvent,
    SERVO_COUNT,
};
pub use menu::*;
use mode::AppMode;
pub use overlay::{DeviceKind, Overlay, PopupConfig, PopupDismiss};
use ratatui::widgets::ListState;
pub use route::{DeviceControlMode, EditField, Route, SelectingField, SelectingKind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// 最近一次设备切换提交的留痕 (E3 = c 的失败 UX 用)
///
/// 服务端 `ServerEvent::Error` 是公共通道; 客户端只把"刚刚提交过切换"
/// 的 Error 识别为设备切换失败, 弹 transient overlay. 其它 Error (USB
/// 断开, 配置反序列化失败等) 走原通道.
#[derive(Debug, Clone, Copy)]
pub struct DeviceSubmitStamp {
    pub at: Instant,
    pub kind: DeviceKind,
}

/// 最近一次设备切换提交的留痕 (1 秒窗口匹配 `ServerEvent::Error`)
const DEVICE_SUBMIT_WINDOW: std::time::Duration = std::time::Duration::from_secs(1);

/// 跨 Route 共享的设备列表缓存
///
/// 进 Settings 时一次性从服务端拉到 `inputs` / `outputs`, picker 与
/// 失败 UX 都从这里读. 跨帧之间 in-place 更新, 不复制.
#[derive(Debug, Clone)]
pub struct DeviceCache {
    pub inputs: Vec<DeviceInfoDto>,
    pub outputs: Vec<DeviceInfoDto>,
    pub loaded_at: Instant,
}

impl Default for DeviceCache {
    fn default() -> Self {
        Self {
            inputs: Vec::new(),
            outputs: Vec::new(),
            loaded_at: Instant::now(),
        }
    }
}

/// tokio runtime, 用于在主循环中调用 .await
pub type Runtime = tokio::runtime::Runtime;

/// 设置项标签(供 EditField::label 使用)
pub const SETTINGS_LABELS: [&str; 4] = ["Wifi名称", "Wifi密码", "麦克风", "扬声器"];

/// 设置项索引常量 — 列表顺序变更时, 引用方必须同步更新
pub const SETTINGS_IDX_WIFI_SSID: usize = 0;
pub const SETTINGS_IDX_WIFI_PASSWORD: usize = 1;
pub const SETTINGS_IDX_SPEECH: usize = 2;
pub const SETTINGS_IDX_OUTPUT: usize = 3;

/// UI 状态
#[derive(Debug)]
pub struct UiState {
    /// 侧边栏 ListState, 高亮当前选中项
    pub menu_state: ListState,
    /// 主循环是否继续
    pub running: bool,
    /// 路由 + 模态组合(取代旧的 5 个 mode bool + left_focused + popup)
    pub mode: AppMode,
}

/// AI 测试页状态(本地 UI 状态)
pub struct AiState {
    pub llm_test_state: LlmTestState,
    pub tts_test_state: TtsTestState,
    pub is_processing: Arc<AtomicBool>,
}

/// 服务端状态的本地镜像
pub struct ServerStateMirror {
    pub robot_connected: bool,
    pub joint_values: [i16; SERVO_COUNT],
    pub joint_selected: usize,
    pub mood: Mood,
    pub last_llm_response: Option<LlmResponse>,
    pub last_screenshot: Option<String>,
    pub volume: i32,
    pub last_error: Option<String>,
    pub net_connected: bool,
    /// 最近一次服务端检测到的人脸位置(用于 UI 可选显示)
    pub last_face: Option<ele_bot_proto::FacePosition>,
}

/// 主应用
pub struct App {
    pub ui: UiState,
    pub ai: AiState,
    pub server: Mutex<ServerStateMirror>,
    pub config: AppConfig,
    pub face_tracking_enabled: bool,
    /// 跨 Route 共享的设备列表 (来自 `*Devices` 事件)
    pub devices: DeviceCache,
    /// 最近一次设备切换提交留痕 (1 秒窗口内 Error 视为切换失败)
    pub last_device_submit: Option<DeviceSubmitStamp>,
    /// tokio runtime
    rt: Runtime,
    /// WebSocket 客户端
    client: Option<Client>,
    #[allow(dead_code)]
    server_url: String,
}

#[allow(dead_code)]
impl App {
    /// 创建并连接服务端
    pub fn new(server_url: &str) -> anyhow::Result<Self> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;

        let client = rt.block_on(async { Client::connect(server_url).await })?;

        let mut menu_state = ListState::default();
        menu_state.select(Some(0));

        let app = Self {
            ui: UiState {
                menu_state,
                running: true,
                mode: AppMode::new(),
            },
            ai: AiState {
                llm_test_state: LlmTestState::default(),
                tts_test_state: TtsTestState::default(),
                is_processing: Arc::new(AtomicBool::new(false)),
            },
            server: Mutex::new(ServerStateMirror {
                robot_connected: false,
                joint_values: [0; SERVO_COUNT],
                joint_selected: 0,
                mood: Mood::Default,
                last_llm_response: None,
                last_screenshot: None,
                volume: 0,
                last_error: None,
                net_connected: true,
                last_face: None,
            }),
            config: AppConfig::default(),
            face_tracking_enabled: false,
            devices: DeviceCache::default(),
            last_device_submit: None,
            rt,
            client: Some(client),
            server_url: server_url.to_string(),
        };

        if let Some(c) = &app.client {
            c.send(ClientMessage::GetConfig);
        }

        Ok(app)
    }

    /// 轮询并处理服务端事件
    pub fn poll_events(&mut self) {
        let Some(client) = &self.client else {
            return;
        };
        let events = self.rt.block_on(async { client.drain().await });
        for evt in events {
            self.apply_event(evt);
        }
    }

    fn apply_event(&mut self, evt: ServerEvent) {
        // 输入/输出设备列表不写 ServerStateMirror, 单独路径处理,
        // 避免与 server 锁冲突 (refresh_picker_after_load 需要 mut self).
        if let ServerEvent::InputDevices { devices } = evt {
            log::info!("received {} input devices", devices.len());
            self.devices.inputs = devices;
            self.devices.loaded_at = Instant::now();
            self.refresh_picker_after_load(SelectingKind::Input);
            return;
        }
        if let ServerEvent::OutputDevices { devices } = evt {
            log::info!("received {} output devices", devices.len());
            self.devices.outputs = devices;
            self.devices.loaded_at = Instant::now();
            self.refresh_picker_after_load(SelectingKind::Output);
            return;
        }
        let mut server = self.server.lock().unwrap();
        match evt {
            ServerEvent::Pong => {}
            ServerEvent::Config { config } => {
                self.config = config;
            }
            ServerEvent::Connection { is_connected } => {
                server.robot_connected = is_connected;
                // 连接建立后, 自动 dismiss "正在连接设备..." 弹窗
                if is_connected {
                    if let Some(Overlay::Popup { .. }) = &self.ui.mode.overlay {
                        self.ui.mode.overlay = None;
                    }
                }
            }
            ServerEvent::JointState { state } => {
                server.joint_values = state.values;
                server.joint_selected = state.selected;
            }
            ServerEvent::JointConfig { .. } => {}
            ServerEvent::Mood { mood } => {
                server.mood = mood;
            }
            ServerEvent::LlmResponse { response } => {
                let mood = response.mood;
                server.last_llm_response = Some(response.clone());
                server.mood = mood;
                self.ai.is_processing.store(false, Ordering::Relaxed);
                if matches!(self.ui.mode.route, Route::LlmTest) {
                    self.ai.llm_test_state.current_mood = Some(mood);
                    self.ai.llm_test_state.output_text = format!("情感: {:?}", mood);
                }
            }
            ServerEvent::LlmProcessing { is_processing } => {
                self.ai
                    .is_processing
                    .store(is_processing, Ordering::Relaxed);
                if is_processing {
                    server.mood = Mood::Loading;
                }
            }
            ServerEvent::ScreenshotSaved { path } => {
                server.last_screenshot = Some(path);
            }
            ServerEvent::Error { message } => {
                log::warn!("server error: {message}");
                // 失败 UX: 若 Error 距上次设备提交 ≤ 1s 且 kind 一致, 弹 transient overlay
                if let Some(stamp) = self.last_device_submit {
                    if stamp.at.elapsed() <= DEVICE_SUBMIT_WINDOW {
                        let kind = stamp.kind;
                        // 取旧设备名 (失败前 server 实际在用的那个)
                        let old_device_name = match kind {
                            DeviceKind::Input => self.config.speech_name.clone(),
                            DeviceKind::Output => self.config.output_device.clone(),
                        };
                        self.ui.mode.overlay = Some(Overlay::DeviceSwitchFailure {
                            kind,
                            old_device_name,
                            detail: message.clone(),
                        });
                        // 标记已消费, 避免被反复弹出 (下一次 Error 不再叠加)
                        self.last_device_submit = None;
                        return;
                    }
                }
                server.last_error = Some(message);
            }
            ServerEvent::Face { position } => {
                // 人脸追踪实际由服务端执行, 这里仅做可选的 UI 展示
                server.last_face = Some(position);
            }
            ServerEvent::CameraResolution { width, height } => {
                log::debug!("camera resolution: {width}x{height}");
            }
            ServerEvent::Volume { value } => {
                server.volume = value;
            }
            // InputDevices / OutputDevices 已在 apply_event 入口短路处理
            ServerEvent::InputDevices { .. } | ServerEvent::OutputDevices { .. } => {}
        }
    }

    fn send_cmd(&self, msg: ClientMessage) {
        if let Some(c) = &self.client {
            c.send(msg);
        }
    }

    pub fn connect_robot(&mut self) {
        // 弹窗常驻, 等 ServerEvent::Connection 回来再 dismiss (apply_event 处理)
        // Esc 可中断, 由 on_dismiss 决定行为
        self.ui.mode.overlay = Some(Overlay::Popup {
            config: PopupConfig::connecting(),
            on_dismiss: PopupDismiss::CancelConnect,
        });
        self.send_cmd(ClientMessage::ConnectRobot);
    }

    pub fn stop_comm_thread(&mut self) {
        self.send_cmd(ClientMessage::DisconnectRobot);
        // 清掉弹窗(包括 Popup::CancelConnect 的"连接中"弹窗)
        if matches!(self.ui.mode.overlay, Some(Overlay::Popup { .. })) {
            self.ui.mode.overlay = None;
        }
    }

    pub fn set_angle(&self, index: usize, angle: f32) {
        if index < SERVO_COUNT {
            self.send_cmd(ClientMessage::SetJoint {
                servo_index: index as u8,
                angle,
            });
        }
    }

    pub fn set_angles(&self, angles: [f32; SERVO_COUNT]) {
        self.send_cmd(ClientMessage::SetJoints { angles });
    }

    pub fn next_servo(&self) {
        self.send_cmd(ClientMessage::SelectServo { delta: 1 });
    }

    pub fn prev_servo(&self) {
        self.send_cmd(ClientMessage::SelectServo { delta: -1 });
    }

    pub fn increase_selected(&self) {
        self.send_cmd(ClientMessage::AdjustSelectedServo { delta: 1 });
    }

    pub fn decrease_selected(&self) {
        self.send_cmd(ClientMessage::AdjustSelectedServo { delta: -1 });
    }

    pub fn set_mood(&self, mood: Mood) {
        self.send_cmd(ClientMessage::SetMood { mood });
    }

    pub fn set_lcd_mode(&self, mode: DisplayMode) {
        self.send_cmd(ClientMessage::SetLcdMode { mode });
    }

    pub fn toggle_face_tracking(&mut self) {
        self.face_tracking_enabled = !self.face_tracking_enabled;
        // 实际的人脸追踪计算在服务端执行
        self.send_cmd(ClientMessage::SetFaceTracking {
            enabled: self.face_tracking_enabled,
        });
        log::info!("Face tracking enabled: {}", self.face_tracking_enabled);
    }

    pub fn send_llm_text(&self, text: String) {
        self.send_cmd(ClientMessage::SendLlmText { text });
    }

    pub fn speak_tts(&self, text: String, speed: f32, streaming: bool) {
        self.send_cmd(ClientMessage::TtsSpeak {
            text,
            speed,
            streaming,
        });
    }

    pub fn take_screenshot(&self) {
        self.send_cmd(ClientMessage::TakeScreenshot);
    }

    pub fn set_config(&self, config: AppConfig) {
        self.send_cmd(ClientMessage::SetConfig { config });
    }

    pub fn is_connected(&self) -> bool {
        self.server.lock().unwrap().robot_connected
    }

    pub fn joint_values(&self) -> [i16; SERVO_COUNT] {
        self.server.lock().unwrap().joint_values
    }

    pub fn joint_selected(&self) -> usize {
        self.server.lock().unwrap().joint_selected
    }

    pub fn quit(&mut self) {
        self.ui.running = false;
    }

    /// 弹出"确认退出"对话框(常驻, 等用户按 Enter 确认或 Esc 取消)
    pub fn confirm_quit(&mut self) {
        self.ui.mode.overlay = Some(Overlay::Popup {
            config: PopupConfig::confirm_quit(),
            on_dismiss: PopupDismiss::ConfirmQuit,
        });
    }

    pub fn next_menu(&mut self) {
        self.select_menu(1);
    }

    pub fn prev_menu(&mut self) {
        self.select_menu(-1);
    }

    /// 侧边栏上下选择。
    /// - 若当前在 Nav: 直接更新 last_entered, 路由不变(已在 Nav)
    /// - 若在某个页面: 切到 Nav 并把 last_entered 设为新选项
    fn select_menu(&mut self, delta: isize) {
        let items = MenuItem::all();
        let len = items.len();
        let current = self.ui.menu_state.selected().unwrap_or(0);
        let new_i = (current as isize + delta).rem_euclid(len as isize) as usize;
        self.ui.menu_state.select(Some(new_i));
        let new_item = items[new_i];
        // 把路由切到 Nav 并记下 last_entered
        self.ui.mode.route = Route::Nav {
            last_entered: new_item,
        };
    }

    /// 进入某个菜单项对应的页面
    pub fn enter_menu(&mut self) {
        let item = self
            .ui
            .menu_state
            .selected()
            .map_or(MenuItem::DeviceStatus, |i| {
                MenuItem::all()[i.min(MenuItem::all().len() - 1)]
            });
        self.ui.mode.route = Route::from(item);
    }

    /// 退到 Nav(从某页面按下 Esc 时)
    pub fn back_to_nav(&mut self) {
        let last = self.ui.mode.route.menu_item();
        self.ui.mode.route = Route::Nav { last_entered: last };
    }

    /// 把当前 DeviceControl 切到 Active(进入子模式)
    pub fn enter_device_control_active(&mut self) {
        if let Route::DeviceControl { mode } = &mut self.ui.mode.route {
            *mode = DeviceControlMode::Active;
        }
    }

    /// 把当前 DeviceControl 切到 Idle
    pub fn enter_device_control_idle(&mut self) {
        if let Route::DeviceControl { mode } = &mut self.ui.mode.route {
            *mode = DeviceControlMode::Idle;
        }
    }

    pub fn settings_item_count(&self) -> usize {
        SETTINGS_LABELS.len()
    }

    pub fn settings_prev(&mut self) {
        if let Route::Settings { selected, .. } = &mut self.ui.mode.route {
            let count = SETTINGS_LABELS.len() as isize;
            *selected = (*selected as isize - 1).rem_euclid(count) as usize;
        }
    }

    pub fn settings_next(&mut self) {
        if let Route::Settings { selected, .. } = &mut self.ui.mode.route {
            let count = SETTINGS_LABELS.len() as isize;
            *selected = (*selected as isize + 1).rem_euclid(count) as usize;
        }
    }

    /// 进入设置项编辑(从 Settings 页面 Enter 时调用)
    ///
    /// 仅对 Wifi 名称 / Wifi 密码 (index 0/1) 生效. 麦克风 / 扬声器 (2/3)
    /// 由 `enter_device_picker` 处理, 不会进入文本编辑模式.
    pub fn begin_settings_edit(&mut self) {
        if let Route::Settings {
            selected,
            editing,
            selecting: _,
        } = &mut self.ui.mode.route
        {
            let initial = match *selected {
                SETTINGS_IDX_WIFI_SSID => self.config.wifi_ssid.clone(),
                SETTINGS_IDX_WIFI_PASSWORD => self.config.wifi_password.clone(),
                // 麦克风 / 扬声器行 (2/3) 走 picker, 此处返回不编辑
                _ => return,
            };
            *editing = Some(EditField::new(
                *selected,
                SETTINGS_LABELS[*selected],
                initial,
            ));
        }
    }

    /// 提交编辑(Enter on EditField overlay)
    pub fn commit_settings_edit(&mut self) {
        if let Route::Settings {
            selected,
            editing,
            selecting: _,
        } = &mut self.ui.mode.route
        {
            if let Some(field) = editing.take() {
                match field.index {
                    SETTINGS_IDX_WIFI_SSID => self.config.wifi_ssid = field.buffer,
                    SETTINGS_IDX_WIFI_PASSWORD => self.config.wifi_password = field.buffer,
                    _ => {}
                }
                *selected = field.index;
            }
        }
        self.set_config(self.config.clone());
    }

    /// 取消编辑(Esc on EditField overlay)
    pub fn cancel_settings_edit(&mut self) {
        if let Route::Settings {
            editing,
            selecting: _,
            ..
        } = &mut self.ui.mode.route
        {
            *editing = None;
        }
    }

    /// 拉取输入/输出设备列表(并发两条 `List*Devices` 消息)
    pub fn refresh_device_lists(&self) {
        self.send_cmd(ClientMessage::ListInputDevices);
        self.send_cmd(ClientMessage::ListOutputDevices);
    }

    /// 进入设备选择器 — 由 `SettingsEvent::EnterPicker` 调用
    ///
    /// - `kind`: 输入 / 输出
    /// - 若本地缓存已就绪 (≥1 个设备), 直接以列表形式打开
    /// - 否则设 `loading=true` 并发 `List*Devices` 请求, 由 `apply_event`
    ///   收到 `*Devices` 后清 loading
    ///
    /// **光标默认对齐**: 打开时若当前配置的设备仍在列表里, 直接落到那条,
    /// 用户 Enter 等于"保持当前选择"; 没匹配上才回 idx 0 (`<系统默认>`).
    /// 避免无脑 Enter 误中系统默认, 把 `name` 清空、`device_id` 也清掉.
    pub fn enter_device_picker(&mut self, kind: SelectingKind) {
        let devices = self.picker_devices(kind).to_vec();
        let loading = devices.is_empty();
        let mut selecting = SelectingField::new(kind);
        selecting.loading = loading;

        let (current_id, current_name) = match kind {
            SelectingKind::Input => (
                self.config.speech_device_id.as_deref(),
                self.config.speech_name.as_str(),
            ),
            SelectingKind::Output => (
                self.config.output_device_id.as_deref(),
                self.config.output_device.as_str(),
            ),
        };
        let matched_idx = current_id
            .and_then(|id| devices.iter().position(|d| d.id == id))
            .or_else(|| {
                if current_name.is_empty() {
                    None
                } else {
                    devices.iter().position(|d| d.name == current_name)
                }
            });
        if let Some(idx) = matched_idx {
            // idx 0 是 `<系统默认>`, 实际设备从 1 开始
            selecting.cursor = idx + 1;
            log::debug!(
                "device picker cursor aligned to current config: idx={}, name={current_name:?}, id={current_id:?}",
                idx + 1
            );
        }

        if let Route::Settings { selecting: s, .. } = &mut self.ui.mode.route {
            *s = Some(selecting.clone());
        }
        self.ui.mode.overlay = Some(Overlay::DevicePicker { selecting, devices });
        if loading {
            self.send_device_list_request(kind);
        }
    }

    /// picker ↑ — wrap, 含 loading 占位
    pub fn picker_up(&mut self) {
        let len = self.picker_overlay_devices_len();
        if len <= 1 {
            return;
        }
        if let Route::Settings {
            selecting: Some(field),
            ..
        } = &mut self.ui.mode.route
        {
            field.cursor = (field.cursor + len - 1) % len;
            if let Some(Overlay::DevicePicker { selecting: s, .. }) = &mut self.ui.mode.overlay {
                s.cursor = field.cursor;
            }
        }
    }

    /// picker ↓ — wrap, 含 loading 占位
    pub fn picker_down(&mut self) {
        let len = self.picker_overlay_devices_len();
        if len <= 1 {
            return;
        }
        if let Route::Settings {
            selecting: Some(field),
            ..
        } = &mut self.ui.mode.route
        {
            field.cursor = (field.cursor + 1) % len;
            if let Some(Overlay::DevicePicker { selecting: s, .. }) = &mut self.ui.mode.overlay {
                s.cursor = field.cursor;
            }
        }
    }

    /// 提交 picker 选择
    ///
    /// - idx 0 → `<系统默认>` (空字符串, id 也置 None)
    /// - idx > 0 → 写 `devices[idx-1].name` 和 `devices[idx-1].id`,
    ///   服务端按 id 优先匹配, name 仅作兜底
    /// - 设 `last_device_submit`, 清 overlay 与 selecting, 发 `SetConfig`
    pub fn confirm_device_picker(&mut self) {
        let kind = if let Route::Settings {
            selecting: Some(s), ..
        } = &self.ui.mode.route
        {
            s.kind
        } else {
            return;
        };
        let devices = self.picker_devices(kind).to_vec();
        let cursor = if let Route::Settings {
            selecting: Some(s), ..
        } = &self.ui.mode.route
        {
            s.cursor
        } else {
            0
        };
        let (chosen_name, chosen_id) = if cursor == 0 {
            (String::new(), None)
        } else {
            match devices.get(cursor - 1) {
                Some(d) => (d.name.clone(), Some(d.id.clone())),
                None => return,
            }
        };
        match kind {
            SelectingKind::Input => {
                self.config.speech_name = chosen_name.clone();
                self.config.speech_device_id = chosen_id.clone();
            }
            SelectingKind::Output => {
                self.config.output_device = chosen_name.clone();
                self.config.output_device_id = chosen_id.clone();
            }
        }
        self.last_device_submit = Some(DeviceSubmitStamp {
            at: Instant::now(),
            kind: kind.into(),
        });
        if let Route::Settings { selecting: s, .. } = &mut self.ui.mode.route {
            *s = None;
        }
        self.ui.mode.overlay = None;
        log::info!("device picker commit: kind={kind:?} name={chosen_name} id={chosen_id:?}");
        self.send_cmd(ClientMessage::SetConfig {
            config: self.config.clone(),
        });
    }

    /// picker Esc — 关闭 overlay 与 selecting, 不写 config
    pub fn cancel_device_picker(&mut self) {
        if let Route::Settings { selecting: s, .. } = &mut self.ui.mode.route {
            *s = None;
        }
        self.ui.mode.overlay = None;
    }

    /// picker R — 重新拉列表, 切 loading=true
    pub fn refresh_device_picker(&mut self) {
        let kind = match (&self.ui.mode.route, &self.ui.mode.overlay) {
            (
                Route::Settings {
                    selecting: Some(s), ..
                },
                Some(Overlay::DevicePicker { .. }),
            ) => s.kind,
            _ => return,
        };
        if let Route::Settings {
            selecting: Some(s), ..
        } = &mut self.ui.mode.route
        {
            s.loading = true;
            s.cursor = 0;
        }
        if let Some(Overlay::DevicePicker { selecting, devices }) = &mut self.ui.mode.overlay {
            selecting.loading = true;
            selecting.cursor = 0;
            devices.clear();
        }
        self.send_device_list_request(kind);
    }

    /// 失败 transient Esc 关闭 — 同时清 `last_device_submit` 防重复弹
    pub fn dismiss_device_failure(&mut self) {
        self.ui.mode.overlay = None;
        self.last_device_submit = None;
    }

    /// Settings 列表模式按 R — 同 `refresh_device_lists`
    pub fn refresh_device_lists_in_settings(&mut self) {
        self.refresh_device_lists();
    }

    // ---- 内部辅助 ----

    fn picker_devices(&self, kind: SelectingKind) -> &[DeviceInfoDto] {
        match kind {
            SelectingKind::Input => &self.devices.inputs,
            SelectingKind::Output => &self.devices.outputs,
        }
    }

    fn picker_overlay_devices_len(&self) -> usize {
        if let Some(Overlay::DevicePicker { devices, .. }) = &self.ui.mode.overlay {
            devices.len() + 1 // +1 为 `<系统默认>`
        } else {
            0
        }
    }

    fn send_device_list_request(&self, kind: SelectingKind) {
        match kind {
            SelectingKind::Input => self.send_cmd(ClientMessage::ListInputDevices),
            SelectingKind::Output => self.send_cmd(ClientMessage::ListOutputDevices),
        }
    }

    /// 收到 `*Devices` 事件后, 若 picker 正在等待同 kind 列表, 替换数据并清 loading
    fn refresh_picker_after_load(&mut self, kind: SelectingKind) {
        let need_refresh = matches!(
            (&self.ui.mode.route, &self.ui.mode.overlay),
            (
                Route::Settings {
                    selecting: Some(s),
                    ..
                },
                Some(Overlay::DevicePicker { .. }),
            ) if s.kind == kind && s.loading
        );
        if !need_refresh {
            return;
        }
        let devices = self.picker_devices(kind).to_vec();
        // 保留旧 cursor (E2 = a): 若 cursor 仍在范围则保持, 否则回 idx 0
        let prev_cursor =
            if let Some(Overlay::DevicePicker { selecting, .. }) = &self.ui.mode.overlay {
                selecting.cursor
            } else {
                0
            };
        let new_len = devices.len() + 1;
        let new_cursor = if prev_cursor < new_len {
            prev_cursor
        } else {
            0
        };
        if let Route::Settings {
            selecting: Some(s), ..
        } = &mut self.ui.mode.route
        {
            s.loading = false;
            s.cursor = new_cursor;
        }
        if let Some(Overlay::DevicePicker {
            selecting,
            devices: d,
        }) = &mut self.ui.mode.overlay
        {
            selecting.loading = false;
            selecting.cursor = new_cursor;
            *d = devices;
        }
    }

    #[allow(dead_code)]
    pub fn load_image_from_file(&mut self, _path: &str) -> anyhow::Result<()> {
        Ok(())
    }

    /// LLM 动作由服务端执行, 此处保留接口
    #[allow(dead_code)]
    pub fn execute_actions(&self, _actions: &[Action]) {}

    pub fn poll_voice_input(&mut self) {
        if self.ai.is_processing.load(Ordering::Relaxed) {
            let mut server = self.server.lock().unwrap();
            server.mood = Mood::Loading;
        }
    }
}
