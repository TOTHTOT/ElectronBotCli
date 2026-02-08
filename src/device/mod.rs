// #[allow(dead_code)]
pub mod image;
pub mod types;

pub use image::ImageProcessor;
pub use types::{JointConfig, BUFFER_COUNT};

/// 查找 USB 设备
pub fn find_usb_device() -> Option<(u16, u16)> {
    let devices = rusb::devices().ok()?;
    for device in devices.iter() {
        let desc = device.device_descriptor().ok()?;
        // ElectronBot device
        if desc.vendor_id() == 0x1001 && desc.product_id() == 0x8023 {
            return Some((desc.vendor_id(), desc.product_id()));
        }
    }
    None
}

/// 检查设备是否连接
pub fn is_device_connected() -> bool {
    find_usb_device().is_some()
}

/// 列出可用端口（兼容旧接口）
#[allow(dead_code)]
pub fn list_ports() -> Vec<String> {
    if is_device_connected() {
        vec!["USB Device".to_string()]
    } else {
        Vec::new()
    }
}
