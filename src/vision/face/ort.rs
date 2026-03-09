//! ONNX 人脸检测后端

use super::detector::{self, FaceDetectionResult, FaceDetectorTrait};
use std::path::PathBuf;

pub struct OrtFaceDetector {
    session: ort::session::Session,
    input_width: u32,
    input_height: u32,
    conf_threshold: f32,
}

impl OrtFaceDetector {
    pub fn new(model_path: PathBuf) -> anyhow::Result<Self> {
        let session = ort::session::Session::builder()?.commit_from_file(model_path)?;
        Ok(Self {
            session,
            input_width: 640,
            input_height: 640,
            conf_threshold: 0.35,
        })
    }
}

impl FaceDetectorTrait for OrtFaceDetector {
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

        let tensor = ort::value::Tensor::from_array((
            vec![1, 3, self.input_height as i32, self.input_width as i32],
            input_data,
        ))?;
        let outputs = self.session.run(ort::inputs!["images" => tensor])?;

        let output_slice = Self::extract_output(&outputs)?;
        Ok(detector::postprocess_output(
            &output_slice,
            scale,
            width,
            height,
            self.conf_threshold,
        ))
    }
}

// ========== 私有方法 ==========

impl OrtFaceDetector {
    fn extract_output(outputs: &ort::session::SessionOutputs) -> anyhow::Result<Vec<f32>> {
        let output = &outputs[0];
        let arr = output.try_extract_array::<f32>()?;
        let view = arr.view();
        Ok(view.as_slice().unwrap_or(&[]).to_vec())
    }
}
