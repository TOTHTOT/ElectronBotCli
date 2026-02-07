// app模块, 负责界面调度以及实际运行功能

pub mod constants;
#[allow(dead_code)]
pub mod display;
#[allow(dead_code)]
pub mod eyes;
pub mod menu;
pub mod servo;

// 导出类型
pub use constants::*;
pub use display::*;
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
    pub display_controller: DisplayController,
    pub port_select_popup: PortSelectPopup,
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
            display_controller: DisplayController::new(),
            port_select_popup: PortSelectPopup::new(),
        }
    }

    /// 发送帧数据 (240x240，分4块发送)
    pub fn send_frame(&mut self) {
        let mut pixels = vec![0u8; FRAME_WIDTH * FRAME_HEIGHT * 3];
        self.display_controller.generate_pixels(&mut pixels);
        let joint = self.servo_state.to_joint_config();
        if self.device.is_connected() {
            match self.device.sync_frame(&pixels, &joint) {
                Ok(_angles) => {
                    // log::info!("SYNC_OK angles=[{:.2}, {:.2}, {:.2}, {:.2}, {:.2}, {:.2}]",
                    //     angles[0], angles[1], angles[2], angles[3], angles[4], angles[5]);
                }
                Err(e) => {
                    log::warn!("通信错误: {e}");
                }
            }
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
            log::error!("连接失败: {e}");
        } else {
            log::info!("已连接到 {port_name}");
        }
    }

    pub fn disconnect_device(&mut self) {
        self.device.disconnect();
        log::info!("设备已断开连接");
    }

    /// 从文件加载图片并设置为静态显示
    pub fn load_image_from_file(&mut self, path: &str) -> bool {
        let mut processor = ImageProcessor::new();
        match processor.load_from_file(path) {
            Ok(image_data) => {
                self.display_controller.set_image(image_data);
                true
            }
            Err(e) => {
                log::warn!("加载图片失败: {e}");
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

/// 端口选择弹窗
pub struct PortSelectPopup {
    pub visible: bool,
    pub ports: Vec<String>,
    pub selected_index: usize,
}

impl PortSelectPopup {
    pub fn new() -> Self {
        Self {
            visible: false,
            ports: Vec::new(),
            selected_index: 0,
        }
    }

    pub fn show(&mut self) {
        self.ports = CdcDevice::list_ports();
        self.selected_index = 0;
        self.visible = true;
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn next(&mut self) {
        if !self.ports.is_empty() {
            self.selected_index = (self.selected_index + 1) % self.ports.len();
        }
    }

    pub fn prev(&mut self) {
        if !self.ports.is_empty() {
            self.selected_index = self.selected_index.saturating_sub(1);
        }
    }

    pub fn selected_port(&self) -> Option<&str> {
        self.ports.get(self.selected_index).map(|s| s.as_str())
    }

    pub fn len(&self) -> usize {
        self.ports.len()
    }
}

impl Default for PortSelectPopup {
    fn default() -> Self {
        Self::new()
    }
}
