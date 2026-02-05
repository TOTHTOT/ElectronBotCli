use std::io::{self, Write};
use std::time::Duration;

pub const FRAME_SIZE: usize = 60 * 240 * 3 + 32; // 43232 bytes
pub const BUFFER_COUNT: usize = 2;

/// CDC 设备连接状态
#[derive(Debug, Clone, PartialEq)]
pub enum DeviceState {
    Disconnected,
    Connected(String),
    Error(String),
}

/// 帧头部结构 (32 bytes)
#[repr(C, packed)]
#[derive(Clone)]
pub struct FrameHeader {
    pub magic: u32,        // 0xAAAAEEEE
    pub width: u32,
    pub height: u32,
    pub x: u32,
    pub y: u32,
    pub format: u32,       // 0: RGB565, 1: RGB888
    pub flag: u32,
    pub reserved: [u8; 12],
}

/// 帧数据结构
#[derive(Clone)]
pub struct FrameData {
    pub header: FrameHeader,
    pub pixels: Vec<u8>,    // 60 * 240 * 3 = 43200 bytes
}

impl Default for FrameData {
    fn default() -> Self {
        Self {
            header: FrameHeader {
                magic: 0xAAAAEEEE_u32.to_le(),
                width: 240,
                height: 60,
                x: 0,
                y: 0,
                format: 1, // RGB888
                flag: 0,
                reserved: [0u8; 12],
            },
            pixels: vec![0u8; 60 * 240 * 3],
        }
    }
}

impl FrameData {
    /// 生成完整的帧数据（包含头部 + 像素数据）
    pub fn to_bytes(&self) -> Vec<u8> {
        let header_bytes = self.header_as_bytes();
        let mut frame = Vec::with_capacity(FRAME_SIZE);
        frame.extend_from_slice(&header_bytes);
        frame.extend_from_slice(&self.pixels);
        frame
    }

    fn header_as_bytes(&self) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        bytes[0..4].copy_from_slice(&self.header.magic.to_le_bytes());
        bytes[4..8].copy_from_slice(&self.header.width.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.header.height.to_le_bytes());
        bytes[12..16].copy_from_slice(&self.header.x.to_le_bytes());
        bytes[16..20].copy_from_slice(&self.header.y.to_le_bytes());
        bytes[20..24].copy_from_slice(&self.header.format.to_le_bytes());
        bytes[24..28].copy_from_slice(&self.header.flag.to_le_bytes());
        bytes
    }
}

/// CDC 设备连接器
pub struct CdcDevice {
    port: Option<Box<dyn serialport::SerialPort>>,
    state: DeviceState,
}

impl CdcDevice {
    pub fn new() -> Self {
        Self {
            port: None,
            state: DeviceState::Disconnected,
        }
    }

    /// 获取当前连接状态
    pub fn state(&self) -> &DeviceState {
        &self.state
    }

    /// 列出可用的串口
    pub fn list_ports() -> Vec<String> {
        serialport::available_ports()
            .unwrap_or_default()
            .into_iter()
            .map(|p| p.port_name)
            .collect()
    }

    /// 连接到 CDC 设备
    pub fn connect(&mut self, port_name: &str) -> io::Result<()> {
        match serialport::new(port_name, 1_000_000) // 1Mbps
            .timeout(Duration::from_millis(100))
            .open()
        {
            Ok(mut port) => {
                port.write_data_terminal_ready(true).ok();
                self.port = Some(port);
                self.state = DeviceState::Connected(port_name.to_string());
                Ok(())
            }
            Err(e) => {
                self.state = DeviceState::Error(format!("连接失败: {}", e));
                Err(io::Error::new(io::ErrorKind::Other, e))
            }
        }
    }

    /// 断开连接
    pub fn disconnect(&mut self) {
        self.port = None;
        self.state = DeviceState::Disconnected;
    }

    /// 发送帧数据
    pub fn send_frame(&mut self, frame: &FrameData) -> io::Result<usize> {
        if let Some(ref mut port) = self.port {
            let data = frame.to_bytes();
            port.write_all(&data)?;
            Ok(data.len())
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "设备未连接",
            ))
        }
    }

    /// 发送双缓冲数据
    // pub fn send_double_buffer(&mut self, frames: &[FrameData; 2]) -> io::Result<()> {
    //     for frame in frames {
    //         self.send_frame(frame)?;
    //         std::thread::sleep(Duration::from_millis(1));
    //     }
    //     Ok(())
    // }

    /// 检查是否已连接
    pub fn is_connected(&self) -> bool {
        self.port.is_some()
    }
}

impl Default for CdcDevice {
    fn default() -> Self {
        Self::new()
    }
}
