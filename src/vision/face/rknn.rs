//! RKNN 人脸检测后端 (仅支持 Linux aarch64)

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
use rknn_rs::prelude::{Rknn, RknnInput, RknnTensorFormat, RknnTensorType};

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
use super::detector::{self, FaceDetectionResult, FaceDetectorTrait};

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
use std::path::PathBuf;

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
pub struct RknnFaceDetector {
    rknn: Rknn,
    input_width: u32,
    input_height: u32,
    conf_threshold: f32,
}

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
impl RknnFaceDetector {
    pub fn new(model_path: PathBuf) -> anyhow::Result<Self> {
        log::info!("Loading RKNN model: {:?}", model_path);
        let rknn = Rknn::rknn_init(&model_path)?;
        log::info!("RKNN model loaded successfully");
        Ok(Self {
            rknn,
            input_width: 640,
            input_height: 640,
            conf_threshold: 0.35,
        })
    }
}

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
impl FaceDetectorTrait for RknnFaceDetector {
    fn detect_multiple(
        &mut self,
        image_data: &[u8],
        width: u32,
        height: u32,
    ) -> anyhow::Result<Vec<FaceDetectionResult>> {
        let scale =
            (self.input_width as f32 / width as f32).min(self.input_height as f32 / height as f32);

        let input_data = detector::preprocess_image(
            image_data,
            width,
            height,
            self.input_width,
            self.input_height,
        )?;

        // RKNN 推理
        let mut input = RknnInput {
            index: 0,
            buf: input_data,
            pass_through: false,
            type_: RknnTensorType::Float32,
            fmt: RknnTensorFormat::NCHW,
        };
        self.rknn.input_set(&mut input)?;
        self.rknn.run()?;
        let outputs: Vec<f32> = self.rknn.outputs_get()?;

        if outputs.is_empty() {
            return Ok(Vec::new());
        }

        Ok(detector::postprocess_output(
            &outputs,
            scale,
            width,
            height,
            self.conf_threshold,
        ))
    }
}

// Stub for non-aarch64 linux - never instantiated
#[cfg(not(all(target_os = "linux", target_arch = "aarch64")))]
#[allow(dead_code)]
pub struct RknnFaceDetector;

#[cfg(not(all(target_os = "linux", target_arch = "aarch64")))]
#[allow(dead_code)]
impl RknnFaceDetector {
    pub fn new(_model_path: std::path::PathBuf) -> anyhow::Result<Self> {
        anyhow::bail!("RKNN is only supported on aarch64-linux")
    }
}
