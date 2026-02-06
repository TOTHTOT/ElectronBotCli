use crate::app::constants::{FRAME_CHUNK_SIZE, FRAME_SIZE};
use crate::device::JointConfig;
use anyhow::{Context, Result};
use std::time::Duration;

pub use super::types::DeviceState;

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
    pub fn connect(&mut self, port_name: &str) -> Result<()> {
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
                self.state = DeviceState::Error(format!("连接失败: {e}"));
                anyhow::bail!("连接失败: {e}")
            }
        }
    }

    /// 断开连接
    pub fn disconnect(&mut self) {
        self.port = None;
        self.state = DeviceState::Disconnected;
    }

    /// 发送一帧完整数据
    pub fn sync_frame(&mut self, full_frame: &[u8], config: &JointConfig) -> Result<[f32; 6]> {
        let port = self.port.as_mut().context("未连接")?;

        let mut feedback_angles = [0.0f32; 6];
        let config_bytes = config.to_bytes();
        let mut rx_buffer = [0u8; 32];
        const PACKET_SIZE: usize = 512;

        for p in 0..4 {
            port.read_exact(&mut rx_buffer).context("读取请求失败")?;

            for j in 0..6 {
                let mut b = [0u8; 4];
                b.copy_from_slice(&rx_buffer[1 + j * 4..1 + j * 4 + 4]);
                feedback_angles[j] = f32::from_le_bytes(b);
            }

            let offset = p * FRAME_CHUNK_SIZE;
            let mut sent = 0;
            while sent < FRAME_CHUNK_SIZE {
                let chunk_size = std::cmp::min(PACKET_SIZE, FRAME_CHUNK_SIZE - sent);
                port.write_all(&full_frame[offset + sent..offset + sent + chunk_size])?;
                sent += chunk_size;
            }

            if p == 3 {
                let mut tail_packet = [0u8; 224];
                tail_packet[0..192]
                    .copy_from_slice(&full_frame[offset + 43008..offset + FRAME_SIZE]);
                tail_packet[192..224].copy_from_slice(&config_bytes);
                port.write_all(&tail_packet)?;
            }
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
