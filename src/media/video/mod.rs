//! 视频模块

pub mod capture;
pub mod encode;
pub mod process;
pub mod types;

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
pub mod rga_adapter;

pub use capture::VideoCapture;
// 导出类型供外部使用
#[allow(unused_imports)]
pub use types::{CameraFormat, FrameCache};
