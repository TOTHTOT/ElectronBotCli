//! RKNN 人脸检测后端 (仅支持 Linux aarch64)
use super::detector::{
    draw_hollow_rect_static, nms_filter, FaceDetectionResult, FaceDetectorTrait,
};
use rknn_rs::prelude::{Rknn, RknnInput, RknnOutput, RknnTensorFormat, RknnTensorType};
use std::path::PathBuf;
use std::time::Instant;

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
        // 1. 快速 Letterbox 预处理
        let input_data = preprocess_image_letterbox(
            image_data,
            width,
            height,
            self.input_width,
            self.input_height,
        )?;

        let start = Instant::now();
        // 2. RKNN 推理
        let mut input = RknnInput {
            index: 0,
            buf: input_data,
            pass_through: false, // 让 NPU 处理模型内置的 mean/std
            type_: RknnTensorType::Uint8,
            fmt: RknnTensorFormat::NHWC,
        };

        self.rknn.input_set(&mut input)?;
        self.rknn.run()?;

        let rknn_output: RknnOutput<f32> = self.rknn.outputs_get()?;
        log::debug!("rknn used time: {:?}", start.elapsed());

        if rknn_output.is_empty() {
            return Ok(Vec::new());
        }

        // 3. 后处理（包含坐标还原）
        Ok(postprocess_output(
            &rknn_output.to_vec(),
            width,
            height,
            self.input_width,
            self.input_height,
            self.conf_threshold,
        ))
    }
}

/// 使用 Letterbox 方式将图像放入模型输入框，不进行 CPU 缩放以节省性能
fn preprocess_image_letterbox(
    image_data: &[u8],
    img_width: u32,
    img_height: u32,
    input_width: u32,
    input_height: u32,
) -> anyhow::Result<Vec<u8>> {
    let start = Instant::now();
    let mut canvas = vec![0u8; (input_width * input_height * 3) as usize];

    let x_offset = (input_width.saturating_sub(img_width)) / 2;
    let y_offset = (input_height.saturating_sub(img_height)) / 2;

    let src_stride = (img_width * 3) as usize;
    let dst_stride = (input_width * 3) as usize;
    let x_offset_bytes = (x_offset * 3) as usize;

    // 逐行拷贝内存
    for y in 0..(img_height as usize).min(input_height as usize) {
        let src_start = y * src_stride;
        let dst_start = (y + y_offset as usize) * dst_stride + x_offset_bytes;

        // 核心提速：copy_from_slice 是 CPU 拷贝的最快路径
        canvas[dst_start..dst_start + src_stride].copy_from_slice(&image_data[src_start..src_start + src_stride]);
    }

    log::debug!("preprocess_image (Letterbox) used time: {:?}", start.elapsed());
    Ok(canvas)
}

fn postprocess_output(
    output_slice: &[f32],
    orig_width: u32,
    orig_height: u32,
    model_w: u32,
    model_h: u32,
    conf_threshold: f32,
) -> Vec<FaceDetectionResult> {
    let start = Instant::now();
    let num_anchors = 8400;
    let mut results = Vec::new();

    // 计算预处理时的偏移量，用于坐标还原
    let x_offset = (model_w as f32 - orig_width as f32) / 2.0;
    let y_offset = (model_h as f32 - orig_height as f32) / 2.0;

    for i in 0..num_anchors {
        let score = output_slice[4 * num_anchors + i];

        if score > conf_threshold {
            let x_center = output_slice[i];
            let y_center = output_slice[num_anchors + i];
            let w = output_slice[2 * num_anchors + i];
            let h = output_slice[3 * num_anchors + i];

            // 坐标还原：减去 Letterbox 的黑边偏移，再归一化到原始图像尺寸
            let real_x = (x_center - x_offset - w / 2.0) / orig_width as f32;
            let real_y = (y_center - y_offset - h / 2.0) / orig_height as f32;
            let norm_w = w / orig_width as f32;
            let norm_h = h / orig_height as f32;

            results.push(FaceDetectionResult {
                has_face: true,
                x: real_x,
                y: real_y,
                width: norm_w,
                height: norm_h,
                confidence: score,
            });
        }
    }

    let nms = nms_filter(results, 0.45);
    log::debug!("NMS filter used time: {:?}", start.elapsed());
    nms
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
        draw_hollow_rect_static(result_img.as_mut(), width, height, x, y, w, h, [0, 255, 0]);
    }

    // 保存结果
    let output_path = PathBuf::from("rknn_test_result.png");
    result_img.save(&output_path)?;
    log::info!("Result saved to: {:?}", output_path);

    Ok(())
}
