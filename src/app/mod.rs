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
pub use constants::*;
pub use display::*;
pub use menu::*;
pub use servo::*;

// 导出 eyes 模块的类型
pub use eyes::{Expression, RobotEyes};

use crate::device::{ImageProcessor, JointConfig};
use ratatui::widgets::ListState;
use std::default::Default;
use std::sync::mpsc;
use std::sync::mpsc::SyncSender;

pub type BotRecvType = (Box<Vec<u8>>, JointConfig);
/// 主应用
pub struct App {
    pub menu_state: ListState,
    pub selected_menu: MenuItem,
    pub running: bool,
    pub servo_state: ServoState,
    pub in_servo_mode: bool,
    pub display_controller: DisplayController,
    pub port_select_popup: PortSelectPopup,
    comm_state: Option<comm::CommState>,
    comm_thread: Option<std::thread::JoinHandle<()>>,
    // 使用 Vec<u8> 发送 config_bytes 确保所有权清晰
    comm_tx: Option<SyncSender<BotRecvType>>,
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
            comm_state: None,
            comm_thread: None,
            comm_tx: None,
        }
    }

    /// 启动后台通信线程
    pub fn start_comm_thread(&mut self) {
        self.stop_comm_thread();

        log::info!("Connecting to USB device...");

        let (tx, rx) = mpsc::sync_channel(1);
        let (state, handle) = comm::start_comm_thread(rx);

        self.comm_state = Some(state);
        self.comm_thread = Some(handle);
        self.comm_tx = Some(tx);
    }

    /// 停止后台通信线程
    pub fn stop_comm_thread(&mut self) {
        if let Some(tx) = self.comm_tx.take() {
            drop(tx); // 关闭 sender 以唤醒 receiver
        }
        if let Some(state) = &self.comm_state {
            comm::stop_comm_thread(state);
        }
        if let Some(handle) = self.comm_thread.take() {
            let _ = handle.join();
        }
        self.comm_state = None;
    }

    /// 发送帧数据到通信线程
    pub fn send_frame(&mut self) -> anyhow::Result<()> {
        if let Some(tx) = &self.comm_tx {
            let mut pixels = Box::new(vec![0u8; FRAME_SIZE]);
            self.display_controller.generate_pixels(&mut pixels);
            let config = self.servo_state.to_joint_config();
            // usb发送较慢, 存在发送阻塞情况, 导致ui卡顿, 这里如果发送失败就继续发送原数据
            tx.try_send((pixels, config))?;
        }
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

    pub fn disconnect_device(&mut self) {
        self.stop_comm_thread();
        log::info!("Device disconnected");
    }

    /// 检查是否已连接
    pub fn is_connected(&self) -> bool {
        self.comm_state.is_some()
    }

    /// 从文件加载图片并设置为静态显示
    pub fn load_image_from_file(&mut self, path: &str) -> anyhow::Result<()> {
        let mut processor = ImageProcessor::new();
        let image_data = processor.load_from_file(path)?;  // 处理图片到 frame_buffer
        self.display_controller.set_image(&image_data);
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
#[allow(dead_code)]
pub struct PortSelectPopup {
    pub visible: bool,
}

impl PortSelectPopup {
    pub fn new() -> Self {
        Self { visible: false }
    }

    #[allow(dead_code)]
    pub fn show(&mut self) {
        self.visible = true;
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    #[allow(dead_code)]
    pub fn next(&mut self) {}
    #[allow(dead_code)]
    pub fn prev(&mut self) {}
}

impl Default for PortSelectPopup {
    fn default() -> Self {
        Self::new()
    }
}
