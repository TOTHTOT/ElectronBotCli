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
    Action, AppConfig, ClientMessage, DisplayMode, LlmResponse, Mood, ServerEvent, SERVO_COUNT,
};
pub use menu::*;
use mode::AppMode;
pub use overlay::{Overlay, PopupConfig, PopupDismiss};
pub use route::{DeviceControlMode, EditField, Route};
use ratatui::widgets::ListState;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// tokio runtime, 用于在主循环中调用 .await
pub type Runtime = tokio::runtime::Runtime;

/// 设置项标签(供 EditField::label 使用)
pub const SETTINGS_LABELS: [&str; 3] = ["Wifi名称", "Wifi密码", "麦克风名称"];

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
                self.ai.is_processing.store(is_processing, Ordering::Relaxed);
                if is_processing {
                    server.mood = Mood::Loading;
                }
            }
            ServerEvent::ScreenshotSaved { path } => {
                server.last_screenshot = Some(path);
            }
            ServerEvent::Error { message } => {
                log::warn!("server error: {message}");
                server.last_error = Some(message);
            }
            ServerEvent::Face { position } => {
                // 人脸追踪实际由服务端执行, 这里仅做可选的 UI 展示
                server.last_face = Some(position);
            }
            ServerEvent::CameraResolution { width, height } => {
                log::debug!("camera resolution: {width}x{height}");
            }
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
        if matches!(
            self.ui.mode.overlay,
            Some(Overlay::Popup { .. })
        ) {
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
        let item = self.ui.menu_state.selected().map_or(MenuItem::DeviceStatus, |i| {
            MenuItem::all()[i.min(MenuItem::all().len() - 1)]
        });
        self.ui.mode.route = Route::from(item);
    }

    /// 退到 Nav(从某页面按下 Esc 时)
    pub fn back_to_nav(&mut self) {
        let last = self.ui.mode.route.menu_item();
        self.ui.mode.route = Route::Nav {
            last_entered: last,
        };
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
    pub fn begin_settings_edit(&mut self) {
        if let Route::Settings { selected, editing } = &mut self.ui.mode.route {
            let initial = match *selected {
                0 => self.config.wifi_ssid.clone(),
                1 => self.config.wifi_password.clone(),
                2 => self.config.speech_name.clone(),
                _ => String::new(),
            };
            *editing = Some(EditField::new(*selected, SETTINGS_LABELS[*selected], initial));
        }
    }

    /// 提交编辑(Enter on EditField overlay)
    pub fn commit_settings_edit(&mut self) {
        if let Route::Settings { selected, editing } = &mut self.ui.mode.route {
            if let Some(field) = editing.take() {
                match field.index {
                    0 => self.config.wifi_ssid = field.buffer,
                    1 => self.config.wifi_password = field.buffer,
                    2 => self.config.speech_name = field.buffer,
                    _ => {}
                }
                *selected = field.index;
            }
        }
        self.set_config(self.config.clone());
    }

    /// 取消编辑(Esc on EditField overlay)
    pub fn cancel_settings_edit(&mut self) {
        if let Route::Settings { editing, .. } = &mut self.ui.mode.route {
            *editing = None;
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
