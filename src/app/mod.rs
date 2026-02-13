pub mod config;
/// app模块, 负责界面调度以及实际运行功能
pub mod menu;

use crate::robot::{self, CommState, DisplayMode, Joint, JointConfig, Lcd};

// 导出菜单
pub use menu::*;

use crate::voice::VoiceManager;
use electron_bot::{FRAME_HEIGHT, FRAME_WIDTH};
use ratatui::widgets::ListState;
use std::sync::mpsc;
use std::sync::mpsc::SyncSender;

pub type BotRecvType = (Vec<u8>, JointConfig);

/// 主应用
pub struct App {
    pub menu_state: ListState,
    pub selected_menu: MenuItem,
    pub running: bool,
    pub joint: Joint,
    pub in_servo_mode: bool,
    pub in_settings: bool,
    pub settings_selected: usize,
    pub in_edit_mode: bool,
    pub edit_buffer: String,
    pub config: config::AppConfig,
    pub lcd: Lcd,
    pub popup: Popup,
    pub voice_manager: VoiceManager,
    pub left_focused: bool, // true=侧边栏有焦点，false=右侧内容有焦点
    comm_state: Option<CommState>,
    comm_thread: Option<std::thread::JoinHandle<()>>,
    comm_tx: Option<SyncSender<BotRecvType>>,
}

#[allow(dead_code)]
impl App {
    pub fn new(voice_manager: VoiceManager) -> Self {
        let mut menu_state = ListState::default();
        menu_state.select(Some(0));

        let lcd = Lcd::new();
        let config = config::AppConfig::load();
        Self {
            menu_state,
            selected_menu: MenuItem::DeviceStatus,
            running: true,
            joint: Joint::new(),
            in_servo_mode: false,
            in_settings: false,
            settings_selected: 0,
            in_edit_mode: false,
            edit_buffer: String::new(),
            config,
            lcd,
            popup: Popup::new(),
            voice_manager,
            left_focused: true, // 默认侧边栏有焦点
            comm_state: None,
            comm_thread: None,
            comm_tx: None,
        }
    }

    /// 连接机器人
    pub fn connect_robot(&mut self) {
        self.stop_comm_thread();
        self.popup.show_connecting();

        log::info!("Connecting to robot...");
        let (tx, rx) = mpsc::sync_channel(1);
        match robot::start_comm_thread(rx) {
            Ok((state, handle)) => {
                self.comm_state = Some(state);
                self.comm_thread = Some(handle);
                self.comm_tx = Some(tx);
                log::info!("Successfully connected to robot...");
            }
            Err(e) => {
                log::warn!("Failed to start comm thread: {e:?}");
            }
        }
        self.popup.hide();
    }

    /// 断开机器人连接
    pub fn stop_comm_thread(&mut self) {
        if let Some(tx) = self.comm_tx.take() {
            drop(tx);
        }
        if let Some(state) = &self.comm_state {
            robot::stop_comm_thread(state);
        }
        if let Some(handle) = self.comm_thread.take() {
            let _ = handle.join();
        }
        self.comm_state = None;
        self.popup.hide();
    }

    /// 发送帧数据 (原始像素数据)
    pub fn send_frame(&mut self) -> anyhow::Result<()> {
        if let Some(tx) = &self.comm_tx {
            let pixels = self.lcd.frame_vec();
            let config = self.joint.config();
            tx.try_send((pixels, config))?;
        }
        Ok(())
    }

    /// 截图并保存为 BMP 文件
    pub fn take_screenshot(&mut self) -> anyhow::Result<()> {
        let pixels = self.lcd.frame_vec();
        let img = image::RgbImage::from_raw(FRAME_WIDTH as u32, FRAME_HEIGHT as u32, pixels)
            .ok_or_else(|| anyhow::anyhow!("Invalid image dimensions"))?;
        // 生成文件名: screenshot_YYYYMMDD_HHMMSS.bmp
        let now = chrono::Local::now();
        let filename = format!(
            "./assets/images/screenshot/screenshot_{}.bmp",
            now.format("%Y%m%d_%H%M%S")
        );
        img.save(&filename)?;
        log::info!("Screenshot saved to: {filename}");

        Ok(())
    }

    pub fn quit(&mut self) {
        self.running = false;
    }

    pub fn next_menu(&mut self) {
        let items = MenuItem::all();
        let i = match self.menu_state.selected() {
            Some(i) => (i + 1) % items.len(),
            None => 0,
        };
        self.menu_state.select(Some(i));
        self.selected_menu = items[i];
    }

    pub fn prev_menu(&mut self) {
        let items = MenuItem::all();
        let i = match self.menu_state.selected() {
            Some(i) => (i + items.len() - 1) % items.len(),
            None => 0,
        };
        self.menu_state.select(Some(i));
        self.selected_menu = items[i];
    }

    /// 切换左右窗口焦点
    pub fn toggle_focus(&mut self) {
        self.left_focused = !self.left_focused;
    }

    /// 设置项数量
    pub fn settings_item_count(&self) -> usize {
        3 // Wifi名称, Wifi密码, 麦克风名称
    }

    /// 设置模式: 上一项
    pub fn settings_prev(&mut self) {
        let count = self.settings_item_count();
        self.settings_selected = (self.settings_selected + count - 1) % count;
    }

    /// 设置模式: 下一项
    pub fn settings_next(&mut self) {
        let count = self.settings_item_count();
        self.settings_selected = (self.settings_selected + 1) % count;
    }

    /// 保存设置项编辑内容
    pub fn save_settings_edit(&mut self) {
        match self.settings_selected {
            0 => self.config.wifi_ssid = self.edit_buffer.clone(),
            1 => self.config.wifi_password = self.edit_buffer.clone(),
            2 => self.config.speech_name = self.edit_buffer.clone(),
            _ => {}
        }
        if let Err(e) = self.config.save() {
            log::error!("Failed to save settings: {e}");
        }
        self.in_edit_mode = false;
        self.edit_buffer.clear();
    }

    /// 取消设置项编辑
    pub fn cancel_settings_edit(&mut self) {
        self.in_edit_mode = false;
        self.edit_buffer.clear();
    }

    pub fn is_connected(&self) -> bool {
        self.comm_state.is_some()
    }

    pub fn load_image_from_file(&mut self, path: &str) -> anyhow::Result<()> {
        self.lcd.load_image(path)?;
        self.lcd.set_mode(DisplayMode::Static);
        Ok(())
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

/// 通用弹窗
#[derive(Debug, Default)]
pub struct Popup {
    pub visible: bool,
    pub config: PopupConfig,
}

impl Popup {
    pub fn new() -> Self {
        Self {
            visible: false,
            config: PopupConfig::default(),
        }
    }

    /// 显示弹窗
    pub fn show(&mut self) {
        self.visible = true;
    }

    /// 隐藏弹窗
    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// 是否可见
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// 设置配置
    pub fn configure(&mut self, config: PopupConfig) {
        self.config = config;
    }

    /// 快速设置连接中弹窗
    pub fn show_connecting(&mut self) {
        self.configure(PopupConfig {
            title: " 连接设备 ".to_string(),
            content: "正在通过 USB 连接设备...".to_string(),
            width: 40,
            height: 5,
            border_color: ratatui::style::Color::Green,
            bg_color: ratatui::style::Color::DarkGray,
            title_color: ratatui::style::Color::Cyan,
        });
        self.show();
    }
}
