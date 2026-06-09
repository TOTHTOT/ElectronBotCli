//! App 主结构 - 客户端版本
//!
//! 持有 UI 状态 + 网络客户端, 通过 WS 与服务端通信。
//! 不再直接持有任何硬件资源。

pub mod config;
pub mod face_tracker;
pub mod menu;

use crate::app::face_tracker::{calculate_body_adjustment, smooth_adjustment};
use crate::net::Client;
use crate::ui::pages::llm_test::LlmTestState;
use crate::ui::pages::tts_test::TtsTestState;
use ele_bot_proto::{
    Action, AppConfig, ClientMessage, DisplayMode, LlmResponse, Mood, ServerEvent, SERVO_COUNT,
};
pub use menu::*;
use ratatui::widgets::ListState;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// tokio runtime, 用于在主循环中调用 .await
pub type Runtime = tokio::runtime::Runtime;

/// UI 状态
pub struct UiState {
    pub menu_state: ListState,
    pub selected_menu: MenuItem,
    pub running: bool,
    pub left_focused: bool,
    pub in_servo_mode: bool,
    pub in_settings: bool,
    pub settings_selected: usize,
    pub in_edit_settings_mode: bool,
    pub edit_buffer: String,
    pub in_llm_test_mode: bool,
    pub in_tts_test_mode: bool,
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
}

/// 主应用
pub struct App {
    pub ui: UiState,
    pub ai: AiState,
    pub server: Mutex<ServerStateMirror>,
    pub config: AppConfig,
    pub popup: Popup,
    pub face_tracking_enabled: bool,
    last_face_adjustment: i32,
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
                selected_menu: MenuItem::DeviceStatus,
                running: true,
                left_focused: true,
                in_servo_mode: false,
                in_settings: false,
                settings_selected: 0,
                in_edit_settings_mode: false,
                in_llm_test_mode: false,
                in_tts_test_mode: false,
                edit_buffer: String::new(),
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
            }),
            config: AppConfig::default(),
            popup: Popup::new(),
            face_tracking_enabled: false,
            last_face_adjustment: 0,
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
                if self.ui.in_llm_test_mode {
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
                if self.face_tracking_enabled && position.has_face {
                    let target = calculate_body_adjustment(position.x);
                    let smoothed = smooth_adjustment(self.last_face_adjustment, target, 0.3);
                    self.last_face_adjustment = smoothed;
                }
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
        self.popup.show_connecting();
        self.send_cmd(ClientMessage::ConnectRobot);
        self.popup.hide();
    }

    pub fn stop_comm_thread(&mut self) {
        self.send_cmd(ClientMessage::DisconnectRobot);
        self.popup.hide();
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
        if !self.face_tracking_enabled {
            self.last_face_adjustment = 0;
        }
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

    pub fn next_menu(&mut self) {
        self.select_menu(1);
    }

    pub fn prev_menu(&mut self) {
        self.select_menu(-1);
    }

    fn select_menu(&mut self, delta: isize) {
        let items = MenuItem::all();
        let len = items.len();
        let current = self.ui.menu_state.selected().unwrap_or(0);
        let new_i = (current as isize + delta).rem_euclid(len as isize) as usize;
        self.ui.menu_state.select(Some(new_i));
        self.ui.selected_menu = items[new_i];
    }

    pub fn toggle_focus(&mut self) {
        self.ui.left_focused = !self.ui.left_focused;
    }

    pub fn settings_item_count(&self) -> usize {
        3
    }

    pub fn settings_prev(&mut self) {
        self.move_settings(-1);
    }

    pub fn settings_next(&mut self) {
        self.move_settings(1);
    }

    fn move_settings(&mut self, delta: isize) {
        let count = self.settings_item_count();
        self.ui.settings_selected =
            (self.ui.settings_selected as isize + delta).rem_euclid(count as isize) as usize;
    }

    pub fn save_settings_edit(&mut self) {
        match self.ui.settings_selected {
            0 => self.config.wifi_ssid = self.ui.edit_buffer.clone(),
            1 => self.config.wifi_password = self.ui.edit_buffer.clone(),
            2 => self.config.speech_name = self.ui.edit_buffer.clone(),
            _ => {}
        }
        self.set_config(self.config.clone());
        self.ui.in_edit_settings_mode = false;
        self.ui.edit_buffer.clear();
    }

    pub fn cancel_settings_edit(&mut self) {
        self.ui.in_edit_settings_mode = false;
        self.ui.edit_buffer.clear();
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

/// 通用弹窗配置
#[derive(Debug, Clone)]
pub struct PopupConfig {
    pub title: String,
    pub content: String,
    pub width: u16,
    pub height: u16,
    pub border_color: ratatui::style::Color,
    pub bg_color: ratatui::style::Color,
    pub title_color: ratatui::style::Color,
}

impl Default for PopupConfig {
    fn default() -> Self {
        Self {
            title: "弹窗".to_string(),
            content: "".to_string(),
            width: 40,
            height: 5,
            border_color: ratatui::style::Color::Green,
            bg_color: ratatui::style::Color::DarkGray,
            title_color: ratatui::style::Color::Cyan,
        }
    }
}

#[derive(Debug, Default)]
pub struct Popup {
    pub visible: bool,
    pub config: PopupConfig,
}

impl Popup {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn show(&mut self) {
        self.visible = true;
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn show_connecting(&mut self) {
        self.config = PopupConfig {
            title: " 连接设备 ".to_string(),
            content: "正在连接设备...".to_string(),
            ..PopupConfig::default()
        };
        self.show();
    }
}
