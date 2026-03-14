//! RKNN 人脸检测后端 (仅支持 Linux aarch64)
use super::detector::{self, draw_hollow_rect_static, FaceDetectionResult, FaceDetectorTrait};
use rknn_rs::prelude::{Rknn, RknnInput, RknnOutput, RknnTensorFormat, RknnTensorType};
use std::path::PathBuf;

pub struct RknnFaceDetector {
    rknn: Rknn,
    input_width: u32,
    input_height: u32,
    conf_threshold: f32,
}

impl RknnFaceDetector {
    pub fn new(model_path: PathBuf) -> anyhow::Result<Self> {
        log::info!("Loading RKNN model: {:?}", model_path);
        let rknn = Rknn::new(&model_path)?;
        log::info!("RKNN model loaded successfully");
        Ok(Self {
            rknn,
            input_width: 640,
            input_height: 640,
            conf_threshold: 0.35,
        })
    }
}

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
            fmt: RknnTensorFormat::NHWC,
        };
        self.rknn.input_set(&mut input)?;
        self.rknn.run()?;
        let rknn_output: RknnOutput<f32> = self.rknn.outputs_get()?;

        if rknn_output.is_empty() {
            return Ok(Vec::new());
        }

        let outputs: Vec<f32> = rknn_output.to_vec();

        Ok(detector::postprocess_output(
            &outputs,
            scale,
            width,
            height,
            self.conf_threshold,
        ))
    }
}

/// 测试人脸检测功能
pub fn test_face_detection(model_path: &str, test_image_path: &str) -> anyhow::Result<()> {
    let model_path = PathBuf::from(model_path);
    let test_image_path = PathBuf::from(test_image_path);

    log::info!("Testing RKNN face detection");
    log::info!("Model: {:?}", model_path);
    log::info!("Test image: {:?}", test_image_path);

    // 加载图片
    let img = image::open(&test_image_path)?;
    let rgb_img = img.to_rgb8();
    let (width, height) = rgb_img.dimensions();
    log::info!("Image dimensions: {}x{}", width, height);

    // 创建检测器
    let mut detector = RknnFaceDetector::new(model_path)?;

    // 运行检测
    let results = detector.detect_multiple(rgb_img.as_raw(), width, height)?;
    log::info!("Detected {} faces", results.len());

    // 绘制结果 (坐标是归一化的，需要转换为像素坐标)
    let mut result_img = rgb_img.clone();
    for (i, face) in results.iter().enumerate() {
        let x = (face.x * width as f32) as i32;
        let y = (face.y * height as f32) as i32;
        let w = (face.width * width as f32) as u32;
        let h = (face.height * height as f32) as u32;
        log::info!(
            "Face {}: x={}, y={}, w={}, h={}, confidence={:.2}",
            i + 1,
            x,
            y,
            w,
            h,
            face.confidence
        );
        draw_hollow_rect_static(
            result_img.as_mut(),
            width,
            height,
            x,
            y,
            w,
            h,
            [0, 255, 0],
        );
    }

    // 保存结果
    let output_path = PathBuf::from("rknn_test_result.png");
    result_img.save(&output_path)?;
    log::info!("Result saved to: {:?}", output_path);

    Ok(())
}
