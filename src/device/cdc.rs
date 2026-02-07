pub use super::types::DeviceState;
use crate::app::SERVO_COUNT;
use crate::device::JointConfig;
use anyhow::{Context, Result};
use std::time::Duration;

// 协议常量
const CHUNK_COUNT: usize = 4;
const CHUNK_VALID_SIZE: usize = 84; // 每块有效数据大小
const CHUNK_TOTAL_SIZE: usize = 512; // 每块传输大小 (84 * 512 = 43008)
const TAIL_FRAME_SIZE: usize = 192; // 尾部帧数据大小
const TAIL_TOTAL_SIZE: usize = 224; // 尾部总大小

/// CDC 设备连接器
pub struct CdcDevice {
    port: Option<Box<dyn serialport::SerialPort>>,
    state: DeviceState,
    ping_pong_index: u8,
}

impl CdcDevice {
    pub fn new() -> Self {
        Self {
            port: None,
            state: DeviceState::Disconnected,
            ping_pong_index: 0,
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
        match serialport::new(port_name, 8_000_000) // 115200 baud
            .timeout(Duration::from_millis(100))
            .open()
        {
            Ok(mut port) => {
                port.write_data_terminal_ready(true).ok();
                port.write_request_to_send(true).ok();
                std::thread::sleep(Duration::from_millis(50));
                self.port = Some(port);
                self.ping_pong_index = 0;
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

    /// 发送帧数据 (按原协议)
    /// full_frame: 172800 字节的帧数据
    /// config: 32 字节的舵机配置
    pub fn sync_frame(&mut self, full_frame: &[u8], config: &JointConfig) -> Result<[f32; 6]> {
        let port = self.port.as_mut().context("未连接")?;
        let config_bytes = config.to_bytes();
        let mut feedback_angles = [0.0f32; 6];
        self.ping_pong_index = if self.ping_pong_index == 0 { 1 } else { 0 };

        // 发送 4 块数据
        let mut frame_offset = 0;
        for _p in 0..CHUNK_COUNT {
            for i in 0..CHUNK_VALID_SIZE {
                let chunk_offset = frame_offset + i * CHUNK_TOTAL_SIZE;
                port.write_all(&full_frame[chunk_offset..chunk_offset + CHUNK_TOTAL_SIZE])?;
            }
            frame_offset += CHUNK_VALID_SIZE * CHUNK_TOTAL_SIZE;

            let mut tail_packet = [0u8; TAIL_TOTAL_SIZE];
            tail_packet[0..TAIL_FRAME_SIZE]
                .copy_from_slice(&full_frame[frame_offset..frame_offset + TAIL_FRAME_SIZE]);
            tail_packet[TAIL_FRAME_SIZE..TAIL_TOTAL_SIZE].copy_from_slice(&config_bytes);
            port.write_all(&tail_packet)?;

            let mut data = [0u8; 32];
            match port.read_exact(&mut data) {
                Ok(()) => {
                    for j in 0..SERVO_COUNT {
                        let mut b = [0u8; 4];
                        b.copy_from_slice(&data[1 + j * 4..1 + j * 4 + 4]);
                        feedback_angles[j] = f32::from_le_bytes(b);
                    }
                }
                Err(e) => log::warn!("read joint failed: {e}"),
            }
            frame_offset += TAIL_FRAME_SIZE;
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
