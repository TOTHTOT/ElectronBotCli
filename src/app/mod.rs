// app模块, 负责界面调度以及实际运行功能
pub mod constants;
#[allow(dead_code)]
pub mod log;
pub mod menu;
pub mod servo;

// 导出类型
pub use constants::*;
pub use log::*;
pub use menu::*;
pub use servo::*;

use crate::device::{CdcDevice, FrameData, ImageProcessor, FRAME_SIZE};
use ratatui::widgets::ListState;
use std::default::Default;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

// 消息命令
pub enum DeviceCmd {
    SendFrame(FrameData),
    Disconnect,
}

/// 设备发送器
pub struct DeviceSender {
    tx: mpsc::Sender<DeviceCmd>,
    running: Arc<AtomicBool>,
}

impl DeviceSender {
    pub fn new(tx: mpsc::Sender<DeviceCmd>, running: Arc<AtomicBool>) -> Self {
        Self { tx, running }
    }

    pub fn send_frame(&self, frame: FrameData) {
        let _ = self.tx.send(DeviceCmd::SendFrame(frame));
    }

    pub fn disconnect(&self) {
        let _ = self.tx.send(DeviceCmd::Disconnect);
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
    }
}

/// 主应用
pub struct App {
    pub menu_state: ListState,
    pub selected_menu: MenuItem,
    pub running: bool,
    pub device: CdcDevice,
    pub frame_counter: u32,
    pub lcd_buffer: Vec<u8>,
    pub servo_state: ServoState,
    pub in_servo_mode: bool,
    pub device_sender: Option<DeviceSender>,
    pub log_queue: LogQueue,
    image_error_logged: bool, // 标记图片加载错误是否已记录
}

impl App {
    pub fn new() -> Self {
        let mut menu_state = ListState::default();
        menu_state.select(Some(0));

        Self {
            menu_state,
            selected_menu: MenuItem::DeviceStatus,
            running: true,
            device: CdcDevice::new(),
            frame_counter: 0,
            lcd_buffer: vec![0u8; FRAME_SIZE],
            servo_state: ServoState::default(),
            in_servo_mode: false,
            device_sender: None,
            log_queue: LogQueue::new(),
            image_error_logged: false,
        }
    }

    pub fn start_device_thread(&mut self) {
        let (tx, rx) = mpsc::channel();
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();
        let mut device = CdcDevice::new();

        thread::spawn(move || {
            while running_clone.load(Ordering::Relaxed) {
                match rx.recv_timeout(std::time::Duration::from_millis(10)) {
                    Ok(DeviceCmd::SendFrame(frame)) => {
                        let _ = device.send_frame(&frame);
                    }
                    Ok(DeviceCmd::Disconnect) => {
                        device.disconnect();
                    }
                    Err(_) => {}
                }
            }
            device.disconnect();
        });

        self.device_sender = Some(DeviceSender::new(tx, running));
    }

    pub fn send_frame(&mut self, frame: &FrameData) {
        self.update_lcd_buffer(frame);
        if let Some(sender) = &self.device_sender {
            sender.send_frame(frame.clone());
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

    pub fn update_lcd_buffer(&mut self, frame: &FrameData) {
        self.lcd_buffer = frame.to_bytes();
    }

    pub fn create_test_frame(&mut self) -> FrameData {
        let mut frame = FrameData::default();
        self.frame_counter = self.frame_counter.wrapping_add(1);

        let mut processor = ImageProcessor::new();
        match processor.load_from_file("assets/images/tet.png") {
            Ok(image_data) => {
                // 图片加载成功，重置错误标记
                self.image_error_logged = false;
                for row in 0..FRAME_HEIGHT {
                    let src_offset = row * ROW_SIZE;
                    let dst_offset = row * ROW_SIZE;
                    frame.pixels[dst_offset..dst_offset + ROW_SIZE]
                        .copy_from_slice(&image_data[src_offset..src_offset + ROW_SIZE]);
                }
            }
            Err(ref e) if !self.image_error_logged => {
                // 只记录一次错误
                self.image_error_logged = true;
                self.log_queue.warn(format!("加载测试图片失败: {e}"));
                self.generate_rainbow(&mut frame);
            }
            Err(_) => {
                // 已经记录过错误，直接生成彩虹
                self.generate_rainbow(&mut frame);
            }
        }

        frame
    }

    fn generate_rainbow(&self, frame: &mut FrameData) {
        for y in 0..FRAME_HEIGHT {
            for x in 0..FRAME_WIDTH {
                let idx = (y * FRAME_WIDTH + x) * 3;
                let hue = ((x + y + self.frame_counter as usize) % 360) as u8;
                let (r, g, b) = hsv_to_rgb(hue as f32, 1.0, 1.0);

                frame.pixels[idx] = r;
                frame.pixels[idx + 1] = g;
                frame.pixels[idx + 2] = b;
            }
        }
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;

    let (r, g, b) = match h as i32 {
        0..=59 => (c, x, 0.0),
        60..=119 => (x, c, 0.0),
        120..=179 => (0.0, c, x),
        180..=239 => (0.0, x, c),
        240..=299 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };

    (
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}
