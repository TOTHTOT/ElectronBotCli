//! Face detection submodule

pub mod detector;
pub mod ort;
pub mod rknn;

pub use detector::{create_face_detector, draw_hollow_rect_static, FaceDetectorTrait};

#[allow(unused_imports)]
pub use detector::FaceDetectionResult;
