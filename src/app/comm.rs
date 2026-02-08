use crate::app::BotRecvType;
use rusb::{Device, DeviceHandle, GlobalContext};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

// ==================== USB 设备常量 ====================
const DEVICE_VID: u16 = 0x1001;
const DEVICE_PID: u16 = 0x8023;
const USB_EP_OUT: u8 = 0x01;
const USB_EP_IN: u8 = 0x81;
const USB_TIMEOUT_MS: u64 = 1000;

// ==================== 帧参数常量 ====================
const FRAME_WIDTH: usize = 240;
const BYTES_PER_PIXEL: usize = 3;
const ROW_SIZE: usize = FRAME_WIDTH * BYTES_PER_PIXEL; // 720 字节/行

const ROWS_PER_ROUND: usize = 60;
const BYTES_PER_ROUND: usize = ROWS_PER_ROUND * ROW_SIZE; // 43200 字节/轮
const ROUND_COUNT: usize = 4;

const USB_PACKET_SIZE: usize = 512;
const TAIL_SIZE: usize = 224; // 192 像素 + 32 config
const PIXELS_IN_TAIL: usize = 192;

// ==================== USB 通信 ====================

/// USB 通信上下文
pub struct UsbComm {
    handle: DeviceHandle<GlobalContext>,
}

impl UsbComm {
    /// 打开 USB 设备
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

    /// 发送使能信号（224 字节心跳）
    pub fn send_enable(&mut self) -> Result<(), rusb::Error> {
        let mut enable = [0u8; 224];
        enable[0] = 1;
        self.write_all(USB_EP_OUT, &enable)
    }

    /// 接收设备请求（32 字节）
    pub fn receive_request(&mut self) -> Result<[u8; 32], rusb::Error> {
        let mut buf = [0u8; 32];
        self.read_exact(USB_EP_IN, &mut buf)?;
        Ok(buf)
    }

    /// 发送尾部数据（192 像素 + 32 config）
    pub fn send_tail(&mut self, pixels: &[u8], config: &[u8; 32], round: usize) -> Result<(), rusb::Error> {
        let mut tail = [0xffu8; TAIL_SIZE];
        let tail_offset = round * BYTES_PER_ROUND + BYTES_PER_ROUND - PIXELS_IN_TAIL;

        tail[..PIXELS_IN_TAIL].copy_from_slice(&pixels[tail_offset..tail_offset + PIXELS_IN_TAIL]);
        tail[PIXELS_IN_TAIL..].copy_from_slice(config);

        self.write_all(USB_EP_OUT, &tail)
    }

    /// 发送一帧数据（包含 4 轮传输）
    pub fn send_frame(&mut self, pixels: &[u8], config: &[u8; 32]) -> Result<(), rusb::Error> {
        for round in 0..ROUND_COUNT {
            // 1. 发送使能信号
            // self.send_enable()?;

            // 2. 接收设备请求
            let request = self.receive_request();
            log::debug!("Round {}: {:?}", round, request);

            // 3. 发送像素数据
            let round_start = round * BYTES_PER_ROUND;
            self.send_pixel_chunks(pixels, round_start)?;

            // 4. 发送尾部（192 像素 + config）
            self.send_tail(pixels, config, round)?;
        }
        Ok(())
    }

    /// 分包发送像素数据
    fn send_pixel_chunks(&mut self, pixels: &[u8], start: usize) -> Result<(), rusb::Error> {
        let mut sent = 0;
        while sent < BYTES_PER_ROUND {
            let chunk = if sent + USB_PACKET_SIZE <= BYTES_PER_ROUND {
                USB_PACKET_SIZE
            } else {
                BYTES_PER_ROUND - sent
            };
            let offset = start + sent;
            self.write_all(USB_EP_OUT, &pixels[offset..offset + chunk])?;
            sent += chunk;
        }
        Ok(())
    }

    /// 写入所有数据（确保完整传输）
    fn write_all(&mut self, endpoint: u8, data: &[u8]) -> Result<(), rusb::Error> {
        let mut written = 0;
        while written < data.len() {
            let n = self.handle.write_bulk(endpoint, &data[written..], Duration::from_millis(USB_TIMEOUT_MS))?;
            written += n;
        }
        Ok(())
    }

    /// 读取完整数据（确保完整接收）
    fn read_exact(&mut self, endpoint: u8, buf: &mut [u8]) -> Result<(), rusb::Error> {
        let mut read = 0;
        while read < buf.len() {
            let n = self.handle.read_bulk(endpoint, &mut buf[read..], Duration::from_millis(USB_TIMEOUT_MS))?;
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

/// 启动通信线程
pub fn start_comm_thread(rx: mpsc::Receiver<BotRecvType>) -> (CommState, thread::JoinHandle<()>) {
    let running = Arc::new(AtomicBool::new(true));
    let state = CommState {
        running: running.clone(),
    };

    let handle = thread::spawn(move || {
        match UsbComm::open() {
            Ok(mut usb) => {
                log::info!("USB device opened");
                thread::sleep(Duration::from_millis(100));

                for (pixels, joint) in rx {
                    if !running.load(Ordering::Relaxed) {
                        break;
                    }

                    if let Err(e) = usb.send_frame(&pixels, &joint.to_bytes()) {
                        log::error!("Send frame failed: {e}");
                    }
                }
                log::info!("Communication thread stopped");
            }
            Err(e) => {
                log::error!("Failed to open USB device: {e}");
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
