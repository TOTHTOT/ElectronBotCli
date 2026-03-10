//! Face detection submodule

pub mod detector;
pub mod ort;
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
pub mod rknn;

pub use detector::{create_face_detector, draw_hollow_rect_static, FaceDetectorTrait};

#[allow(unused_imports)]
pub use detector::FaceDetectionResult;
