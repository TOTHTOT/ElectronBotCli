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

/// 各个关节的运动角度
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct JointConfig {
    pub enable: u8,       // 1: 使能, 0: 掉电
    pub angles: [f32; 6], // 6个关节的角度
    pub padding: [u8; 7], // 补齐到 32 字节
}

impl Default for JointConfig {
    fn default() -> Self {
        Self {
            enable: 0,
            angles: [0.0; 6],
            padding: [0u8; 7],
        }
    }
}

impl JointConfig {
    pub fn to_bytes(self) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        bytes[0] = self.enable;
        for i in 0..6 {
            let b = self.angles[i].to_le_bytes();
            bytes[1 + i * 4..1 + i * 4 + 4].copy_from_slice(&b);
        }
        bytes
    }
}

/// 帧数据结构
#[derive(Clone)]
pub struct FrameData {
    pub joint: JointConfig,
    pub pixels: Vec<u8>, // 60 * 240 * 3 = 43200 bytes
}

impl Default for FrameData {
    fn default() -> Self {
        Self {
            joint: JointConfig::default(),
            pixels: vec![0u8; 60 * 240 * 3], // 43200 字节
        }
    }
}
impl FrameData {
    /// 生成完整的帧数据（包含头部 + 像素数据）
    pub fn to_bytes(&self) -> Vec<u8> {
        let joint_bytes = self.joint.to_bytes();
        let mut frame = Vec::with_capacity(FRAME_SIZE);
        frame.extend_from_slice(&joint_bytes);
        frame.extend_from_slice(&self.pixels);
        frame
    }
}

/// CDC 设备连接器
pub struct CdcDevice {
    port: Option<Box<dyn serialport::SerialPort>>,
    state: DeviceState,
}
#[allow(dead_code)]
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
                Err(io::Error::other(e))
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
            Err(io::Error::new(io::ErrorKind::NotConnected, "设备未连接"))
        }
    }

    // /// 发送双缓冲数据
    // pub fn send_double_buffer(&mut self, frames: &[FrameData; 2]) -> io::Result<()> {
    //     for frame in frames {
    //         self.send_frame(frame)?;
    //         std::thread::sleep(Duration::from_millis(1));
    //     }
    //     Ok(())
    // }
    /// 模拟 C++ 的 SyncTask 逻辑
    /// full_frame: 240x240x3 = 172800 字节的原始 RGB 数据
    /// config: 包含使能开关和 6 个角度的指令
    pub fn sync_frame(&mut self, full_frame: &[u8], config: &JointConfig) -> io::Result<[f32; 6]> {
        let port = self
            .port
            .as_mut()
            .ok_or(io::Error::new(io::ErrorKind::NotConnected, "未连接"))?;

        let mut feedback_angles = [0.0f32; 6];
        let config_bytes = config.to_bytes();
        let mut rx_buffer = [0u8; 32];

        for p in 0..4 {
            port.read_exact(&mut rx_buffer)?;

            for j in 0..6 {
                let mut b = [0u8; 4];
                b.copy_from_slice(&rx_buffer[1 + j * 4..1 + j * 4 + 4]);
                feedback_angles[j] = f32::from_le_bytes(b);
            }

            let offset = p * 43200;
            let body_chunk = &full_frame[offset..offset + 43008];
            port.write_all(body_chunk)?;

            let mut tail_packet = [0u8; 224];
            tail_packet[0..192].copy_from_slice(&full_frame[offset + 43008..offset + 43200]);
            tail_packet[192..224].copy_from_slice(&config_bytes);

            port.write_all(&tail_packet)?;
        }

        Ok(feedback_angles)
    }
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
