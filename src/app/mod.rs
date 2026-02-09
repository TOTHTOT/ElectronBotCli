// app模块, 负责界面调度以及实际运行功能

pub mod menu;

use crate::robot::{self, CommState, DisplayMode, Joint, JointConfig, Lcd};

// 导出菜单
pub use menu::*;

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
    pub lcd: Lcd,
    pub comm_popup: CommPopup,
    comm_state: Option<CommState>,
    comm_thread: Option<std::thread::JoinHandle<()>>,
    comm_tx: Option<SyncSender<BotRecvType>>,
}

impl App {
    pub fn new() -> Self {
        let mut menu_state = ListState::default();
        menu_state.select(Some(0));

        let mut lcd = Lcd::new();
        lcd.set_mode(DisplayMode::TestPattern);

        Self {
            menu_state,
            selected_menu: MenuItem::DeviceStatus,
            running: true,
            joint: Joint::new(),
            in_servo_mode: false,
            lcd,
            comm_popup: CommPopup::new(),
            comm_state: None,
            comm_thread: None,
            comm_tx: None,
        }
    }

    /// 启动后台通信线程
    pub fn start_comm_thread(&mut self) {
        self.stop_comm_thread();
        self.comm_popup.show();
        log::info!("Connecting to robot...");

        let (tx, rx) = mpsc::sync_channel(1);
        let (state, handle) = robot::start_comm_thread(rx);

        self.comm_state = Some(state);
        self.comm_thread = Some(handle);
        self.comm_tx = Some(tx);
    }

    /// 停止后台通信线程
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
        self.comm_popup.hide();
    }

    /// 发送帧数据
    pub fn send_frame(&mut self) -> anyhow::Result<()> {
        if let Some(tx) = &self.comm_tx {
            let pixels = self.lcd.frame_vec();
            let config = self.joint.config();
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

    pub fn is_connected(&self) -> bool {
        self.comm_state.is_some()
    }

    pub fn load_image_from_file(&mut self, path: &str) -> anyhow::Result<()> {
        self.lcd.load_image(path)?;
        self.lcd.set_mode(DisplayMode::Static);
        Ok(())
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

/// 通信提示弹窗
#[derive(Debug, Default)]
pub struct CommPopup {
    pub visible: bool,
}

impl CommPopup {
    pub fn new() -> Self {
        Self { visible: false }
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
}
