// app模块, 负责界面调度以及实际运行功能

pub mod comm;
pub mod constants;
#[allow(dead_code)]
pub mod display;
#[allow(dead_code)]
pub mod eyes;
pub mod menu;
pub mod servo;

// 导出类型
pub use comm::{comm_thread_handler, SharedState};
pub use constants::*;
pub use display::*;
pub use menu::*;
pub use servo::*;

// 导出 eyes 模块的类型
pub use eyes::{Expression, RobotEyes};

use crate::device::{self, ImageProcessor};
use ratatui::widgets::ListState;
use std::default::Default;
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// 主应用
pub struct App {
    pub menu_state: ListState,
    pub selected_menu: MenuItem,
    pub running: bool,
    pub servo_state: ServoState,
    pub in_servo_mode: bool,
    pub display_controller: DisplayController,
    pub port_select_popup: PortSelectPopup,
    shared_state: Option<Arc<SharedState>>,
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
            servo_state: ServoState::default(),
            in_servo_mode: false,
            display_controller: DisplayController::new(),
            port_select_popup: PortSelectPopup::new(),
            shared_state: None,
        }
    }

    /// 启动后台通信线程
    pub fn start_comm_thread(&mut self, port_name: &str) {
        self.stop_comm_thread();

        log::info!("Connecting to {port_name}...");

        let shared = Arc::new(SharedState::new(
            vec![0u8; FRAME_SIZE],
            self.servo_state.to_joint_config(),
        ));

        let port_name = port_name.to_string();
        comm_thread_handler(port_name, shared.clone());

        self.shared_state = Some(shared);
    }

    /// 停止后台通信线程
    pub fn stop_comm_thread(&mut self) {
        if let Some(shared) = &self.shared_state {
            shared.running.store(false, Ordering::Relaxed);
        }
        self.shared_state = None;
    }

    /// 构建屏幕要显示的一帧数据并发送
    pub fn build_send_frame(&mut self) {
        if let Some(shared) = &self.shared_state {
            if let Ok(mut p) = shared.pixels.lock() {
                self.display_controller.generate_pixels(&mut p);
            }
            if let Ok(mut c) = shared.config.lock() {
                *c = self.servo_state.to_joint_config();
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

    pub fn disconnect_device(&mut self) {
        self.stop_comm_thread();
        log::info!("Device disconnected");
    }

    /// 检查是否已连接
    pub fn is_connected(&self) -> bool {
        self.shared_state.is_some()
    }

    /// 从文件加载图片并设置为静态显示
    pub fn load_image_from_file(&mut self, path: &str) -> anyhow::Result<()> {
        let mut processor = ImageProcessor::new();
        let image_data = processor.load_from_file(path)?;
        self.display_controller.set_image(image_data);
        Ok(())
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
        self.ports = device::list_ports();
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
