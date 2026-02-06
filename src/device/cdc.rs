use crate::device::JointConfig;
use std::io::{self, Write};
use std::time::Duration;

pub use super::types::{DeviceState, FrameData};

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

    /// 发送一帧完整数据, 参考原先的流程
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
        const PACKET_SIZE: usize = 512;
        const PACKET_COUNT: usize = 84;

        for p in 0..4 {
            port.read_exact(&mut rx_buffer)?;

            for j in 0..6 {
                let mut b = [0u8; 4];
                b.copy_from_slice(&rx_buffer[1 + j * 4..1 + j * 4 + 4]);
                feedback_angles[j] = f32::from_le_bytes(b);
            }

            let offset = p * 43008;
            let mut sent = 0;
            while sent < 43008 {
                let chunk_size = std::cmp::min(PACKET_SIZE, 43008 - sent);
                port.write_all(&full_frame[offset + sent..offset + sent + chunk_size])?;
                sent += chunk_size;
            }

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
