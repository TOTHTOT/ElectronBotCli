#[allow(dead_code)]
pub mod image;
pub mod types;

pub use image::ImageProcessor;
pub use types::{JointConfig, BUFFER_COUNT};

/// 列出可用的串口
pub fn list_ports() -> Vec<String> {
    serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .map(|p| p.port_name)
        .collect()
}
