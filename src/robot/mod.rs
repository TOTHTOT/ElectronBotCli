//! Robot 模块 - ElectronBot 机器人抽象
//!
//! # 结构
//! - [`Robot`] - 机器人主对象，封装 USB 通信
//! - [`Joint`] - 关节控制模块
//! - [`Lcd`] - LCD 显示模块
//!
//! # 使用
//!
//! ```rust
//! use robot::{Robot, Joint, Lcd};
//!
//! let robot = Robot::open()?;
//! robot.send_frame(&pixels, &joint.to_bytes())?;
//! ```

pub mod joint;
pub mod lcd;

pub use joint::{Joint, JointConfig, ServoState, SERVO_COUNT};
pub use lcd::{DisplayMode, Lcd, BUFFER_COUNT, FRAME_SIZE};

use rusb::{Device, DeviceHandle, GlobalContext};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

// ==================== USB 常量 ====================
const DEVICE_VID: u16 = 0x1001;
const DEVICE_PID: u16 = 0x8023;
const USB_EP_OUT: u8 = 0x01;
const USB_EP_IN: u8 = 0x81;
const USB_TIMEOUT_MS: u64 = 1000;

// ==================== 帧参数常量 ====================
const FRAME_WIDTH: usize = 240;
const BYTES_PER_PIXEL: usize = 3;
const ROW_SIZE: usize = FRAME_WIDTH * BYTES_PER_PIXEL;

const ROWS_PER_ROUND: usize = 60;
const BYTES_PER_ROUND: usize = ROWS_PER_ROUND * ROW_SIZE;
const ROUND_COUNT: usize = 4;

const USB_PACKET_SIZE: usize = 512;
const TAIL_SIZE: usize = 224;
const PIXELS_IN_TAIL: usize = 192;

// ==================== Robot 结构体 ====================

/// ElectronBot 机器人
///
/// 封装 USB 通信和帧发送逻辑
pub struct Robot {
    handle: DeviceHandle<GlobalContext>,
}

impl Robot {
    /// 打开并初始化机器人连接
    pub fn open() -> Result<Self, rusb::Error> {
        let device = find_device()?;
        let handle = device.open()?;

        let interface = 0;
        if handle.kernel_driver_active(interface).unwrap_or(false) {
            handle.detach_kernel_driver(interface)?;
        }
        handle.claim_interface(interface)?;

        Ok(Self { handle })
    }

    /// 接收设备请求
    pub fn receive_request(&mut self) -> Result<[u8; 32], rusb::Error> {
        let mut buf = [0u8; 32];
        self.read_exact(USB_EP_IN, &mut buf)?;
        Ok(buf)
    }

    /// 发送一帧数据（像素 + 关节配置）
    pub fn send_frame(&mut self, pixels: &[u8], config: &[u8; 32]) -> Result<(), rusb::Error> {
        for round in 0..ROUND_COUNT {
            // 1. 发送像素数据
            let round_start = round * BYTES_PER_ROUND;
            self.send_pixel_chunks(pixels, round_start)?;

            // 2. 发送尾部（192 像素 + config）
            self.send_tail(pixels, config, round)?;

            // 3. 读取响应
            let _request = self.receive_request();
            log::trace!("Round {}: received", round);
        }
        Ok(())
    }

    /// 分包发送像素数据（84 包 × 512 字节 = 43008 字节）
    fn send_pixel_chunks(&mut self, pixels: &[u8], start: usize) -> Result<(), rusb::Error> {
        const PACKETS_PER_ROUND: usize = 84;
        let mut sent = 0;

        for _ in 0..PACKETS_PER_ROUND {
            let offset = start + sent;
            self.write_all(USB_EP_OUT, &pixels[offset..offset + USB_PACKET_SIZE])?;
            sent += USB_PACKET_SIZE;
        }
        Ok(())
    }

    /// 发送尾部数据
    fn send_tail(
        &mut self,
        pixels: &[u8],
        config: &[u8; 32],
        round: usize,
    ) -> Result<(), rusb::Error> {
        let mut tail = [0xffu8; TAIL_SIZE];
        let tail_offset = round * BYTES_PER_ROUND + BYTES_PER_ROUND - PIXELS_IN_TAIL;

        tail[..PIXELS_IN_TAIL].copy_from_slice(&pixels[tail_offset..tail_offset + PIXELS_IN_TAIL]);
        tail[PIXELS_IN_TAIL..].copy_from_slice(config);

        self.write_all(USB_EP_OUT, &tail)
    }

    /// 完整写入
    fn write_all(&mut self, endpoint: u8, data: &[u8]) -> Result<(), rusb::Error> {
        let mut written = 0;
        while written < data.len() {
            let n = self.handle.write_bulk(
                endpoint,
                &data[written..],
                Duration::from_millis(USB_TIMEOUT_MS),
            )?;
            written += n;
        }
        Ok(())
    }

    /// 完整读取
    fn read_exact(&mut self, endpoint: u8, buf: &mut [u8]) -> Result<(), rusb::Error> {
        let mut read = 0;
        while read < buf.len() {
            let n = self.handle.read_bulk(
                endpoint,
                &mut buf[read..],
                Duration::from_millis(USB_TIMEOUT_MS),
            )?;
            read += n;
        }
        Ok(())
    }
}

/// 查找 USB 设备
fn find_device() -> Result<Device<GlobalContext>, rusb::Error> {
    let devices = rusb::devices()?;
    for device in devices.iter() {
        let desc = device.device_descriptor()?;
        if desc.vendor_id() == DEVICE_VID && desc.product_id() == DEVICE_PID {
            return Ok(device.clone());
        }
    }
    Err(rusb::Error::NoDevice)
}

// ==================== 通信线程管理 ====================

/// 通信线程状态
pub struct CommState {
    pub running: Arc<AtomicBool>,
}

/// 启动后台通信线程
pub fn start_comm_thread(
    rx: std::sync::mpsc::Receiver<(Vec<u8>, JointConfig)>,
) -> (CommState, thread::JoinHandle<()>) {
    let running = Arc::new(AtomicBool::new(true));
    let state = CommState {
        running: running.clone(),
    };

    let handle = thread::spawn(move || {
        match Robot::open() {
            Ok(mut robot) => {
                log::info!("Robot connected");
                thread::sleep(Duration::from_millis(100));

                for (pixels, joint) in rx {
                    if !running.load(Ordering::Relaxed) {
                        break;
                    }
                    if let Err(e) = robot.send_frame(&pixels, &joint.as_bytes()) {
                        log::error!("Send frame failed: {e}");
                    }
                }
                log::info!("Communication stopped");
            }
            Err(e) => {
                log::error!("Failed to connect: {e}");
            }
        }
        running.store(false, Ordering::Relaxed);
    });

    (state, handle)
}

/// 停止通信线程
pub fn stop_comm_thread(state: &CommState) {
    state.running.store(false, Ordering::Relaxed);
}
