use crate::device::{CdcDevice, FrameData, FRAME_SIZE};
use ratatui::widgets::ListState;
use std::default::Default;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

pub const SERVO_COUNT: usize = 6;
pub const SERVO_MIN: i16 = -180;
pub const SERVO_MAX: i16 = 180;

#[derive(Clone, Copy, PartialEq)]
pub enum MenuItem {
    DeviceStatus,
    DeviceControl,
    Settings,
    About,
}

impl MenuItem {
    pub fn title(&self) -> &'static str {
        match self {
            MenuItem::DeviceStatus => "设备状态",
            MenuItem::DeviceControl => "设备控制",
            MenuItem::Settings => "设置",
            MenuItem::About => "关于",
        }
    }

    pub fn all() -> [MenuItem; 4] {
        [
            MenuItem::DeviceStatus,
            MenuItem::DeviceControl,
            MenuItem::Settings,
            MenuItem::About,
        ]
    }
}

/// 舵机状态
#[derive(Clone)]
pub struct ServoState {
    pub values: [i16; SERVO_COUNT], // 6个舵机的角度值 (-180 ~ 180)
    pub selected: usize,            // 当前选中的舵机索引
}

impl Default for ServoState {
    fn default() -> Self {
        Self {
            values: [0; SERVO_COUNT], // 默认 0 度
            selected: 0,
        }
    }
}

impl ServoState {
    /// 获取舵机名称
    pub fn servo_name(index: usize) -> &'static str {
        match index {
            0 => "头部",
            1 => "左臂",
            2 => "右臂",
            3 => "左手",
            4 => "右手",
            5 => "腰部",
            _ => "未知",
        }
    }

    /// 选中的舵机位置+1
    pub fn next_servo(&mut self) {
        self.selected = (self.selected + 1) % SERVO_COUNT;
    }

    /// 选中的舵机位置-1
    pub fn prev_servo(&mut self) {
        self.selected = (self.selected + SERVO_COUNT - 1) % SERVO_COUNT;
    }

    /// 选中的舵机角度增加
    pub fn increase(&mut self) {
        if self.values[self.selected] < SERVO_MAX {
            self.values[self.selected] = (self.values[self.selected] + 1).min(SERVO_MAX);
        }
    }

    /// 选中的舵机角度减少
    pub fn decrease(&mut self) {
        if self.values[self.selected] > SERVO_MIN {
            self.values[self.selected] = (self.values[self.selected] - 1).max(SERVO_MIN);
        }
    }

    /// 选中的舵机角度大幅增加
    pub fn increase_big(&mut self) {
        if self.values[self.selected] < SERVO_MAX {
            self.values[self.selected] = (self.values[self.selected] + 5).min(SERVO_MAX);
        }
    }

    /// 选中的舵机角度大幅减少
    pub fn decrease_big(&mut self) {
        if self.values[self.selected] > SERVO_MIN {
            self.values[self.selected] = (self.values[self.selected] - 5).max(SERVO_MIN);
        }
    }

    /// 将舵机值转换为0-100的百分比
    pub fn to_percent(value: i16) -> u16 {
        ((value - SERVO_MIN) * 100 / (SERVO_MAX - SERVO_MIN)) as u16
    }
}

pub enum DeviceCmd {
    SendFrame(FrameData),
    Disconnect,
}

/// 设备发送器（在线程间传递）
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

pub struct App {
    pub menu_state: ListState,
    pub selected_menu: MenuItem,
    pub running: bool,
    pub device: CdcDevice,
    pub frame_counter: u32,
    pub lcd_buffer: Vec<u8>,
    pub servo_state: ServoState,
    pub in_servo_mode: bool, // 是否在舵机控制模式
    pub device_sender: Option<DeviceSender>,
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
        }
    }

    /// 启动设备发送线程
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
                    Err(_) => {
                        // 超时，继续循环
                    }
                }
            }
            // 清理
            device.disconnect();
        });

        self.device_sender = Some(DeviceSender::new(tx, running));
    }

    /// 通过线程发送帧
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

    /// 连接设备
    pub fn connect_device(&mut self, port_name: &str) {
        if let Err(e) = self.device.connect(port_name) {
            eprintln!("连接失败: {}", e);
        }
    }

    /// 断开设备
    pub fn disconnect_device(&mut self) {
        self.device.disconnect();
    }

    /// 更新 LCD 缓冲区
    pub fn update_lcd_buffer(&mut self, frame: &FrameData) {
        self.lcd_buffer = frame.to_bytes();
    }

    /// 创建测试帧数据
    pub fn create_test_frame(&mut self) -> FrameData {
        let mut frame = FrameData::default();
        self.frame_counter = self.frame_counter.wrapping_add(1);

        // 生成测试图案（彩虹效果）
        let width = 240usize;
        let height = 60usize;

        for y in 0..height {
            for x in 0..width {
                let idx = (y * width + x) * 3;
                let hue = ((x + y + self.frame_counter as usize) % 360) as u8;
                let (r, g, b) = hsv_to_rgb(hue as f32, 1.0, 1.0);

                frame.pixels[idx] = r;
                frame.pixels[idx + 1] = g;
                frame.pixels[idx + 2] = b;
            }
        }

        frame
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

/// HSV 转 RGB
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
