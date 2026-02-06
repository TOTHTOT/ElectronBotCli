// app模块, 负责界面调度以及实际运行功能
pub mod constants;
#[allow(dead_code)]
pub mod display;
#[allow(dead_code)]
pub mod eyes;
pub mod log;
pub mod menu;
pub mod servo;

// 导出类型
pub use constants::*;
pub use display::*;
pub use log::*;
pub use menu::*;
pub use servo::*;

// 导出 eyes 模块的类型
pub use eyes::{Expression, RobotEyes};

use crate::device::{CdcDevice, ImageProcessor};
use ratatui::widgets::ListState;
use std::default::Default;

/// 主应用
pub struct App {
    pub menu_state: ListState,
    pub selected_menu: MenuItem,
    pub running: bool,
    pub device: CdcDevice,
    pub servo_state: ServoState,
    pub in_servo_mode: bool,
    pub log_queue: LogQueue,
    pub display_controller: DisplayController,
    image_error_logged: bool,
}

#[allow(dead_code)]
impl App {
    pub fn new() -> Self {
        let mut menu_state = ListState::default();
        menu_state.select(Some(0));

        Self {
            menu_state,
            selected_menu: MenuItem::DeviceStatus,
            running: true,
            device: CdcDevice::new(),
            servo_state: ServoState::default(),
            in_servo_mode: false,
            log_queue: LogQueue::new(),
            display_controller: DisplayController::new(),
            image_error_logged: false,
        }
    }

    /// 发送帧数据 (240x240，分4块发送)
    pub fn send_frame(&mut self) {
        let mut pixels = vec![0u8; FRAME_WIDTH * FRAME_HEIGHT * 3];
        self.display_controller.generate_pixels(&mut pixels);
        let joint = self.servo_state.to_joint_config();
        if self.device.is_connected() {
            let _ = self.device.sync_frame(&pixels, &joint);
        }
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

    pub fn connect_device(&mut self, port_name: &str) {
        if let Err(e) = self.device.connect(port_name) {
            self.log_queue.error(format!("连接失败: {e}"));
        } else {
            self.log_queue.info(format!("已连接到 {port_name}"));
        }
    }

    pub fn disconnect_device(&mut self) {
        self.device.disconnect();
    }

    /// 从文件加载图片并设置为静态显示
    pub fn load_image_from_file(&mut self, path: &str) -> bool {
        let mut processor = ImageProcessor::new();
        match processor.load_from_file(path) {
            Ok(image_data) => {
                self.image_error_logged = false;
                self.display_controller.set_image(image_data);
                true
            }
            Err(e) => {
                if !self.image_error_logged {
                    self.image_error_logged = true;
                    self.log_queue.warn(format!("加载图片失败: {e}"));
                }
                false
            }
        }
    }

    /// 设置显示模式
    pub fn set_display_mode(&mut self, mode: DisplayMode) {
        self.display_controller.set_mode(mode);
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}
