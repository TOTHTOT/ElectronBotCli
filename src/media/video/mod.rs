//! 视频模块

pub mod capture;
pub mod encode;
pub mod process;
pub mod types;

pub use capture::VideoCapture;
// 导出类型供外部使用
#[allow(unused_imports)]
pub use types::{CameraFormat, FrameCache};
