//! 兼容原有接口的 RetinaFace 增强后端
use super::detector::{nms_filter, FaceDetectionResult, FaceDetectorTrait};
use rknn_rs::prelude::{Rknn, RknnInput, RknnOutput, RknnTensorFormat, RknnTensorType};
use std::path::PathBuf;
use std::time::Instant;

pub struct RknnFaceDetector {
    rknn: Rknn,
    input_width: u32,
    input_height: u32,
    conf_threshold: f32,
    // 新增：预生成的先验框，避免每次推理都计算
    priors: Vec<[f32; 4]>,
}

impl RknnFaceDetector {
    pub fn new(model_path: PathBuf) -> anyhow::Result<Self> {
        let rknn = Rknn::new(&model_path)?;
        let input_width = 320; // 建议 RK3566 用 320
        let input_height = 320;

        // 初始化时一次性生成 priors
        let priors = generate_priors(input_width, input_height);

        Ok(Self {
            rknn,
            input_width,
            input_height,
            conf_threshold: 0.5,
            priors,
        })
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
        canvas[dst_start..dst_start + src_stride]
            .copy_from_slice(&image_data[src_start..src_start + src_stride]);
    }

    log::debug!(
        "preprocess_image (Letterbox) used time: {:?}",
        start.elapsed()
    );
    Ok(canvas)
}
impl FaceDetectorTrait for RknnFaceDetector {
    fn detect_multiple(
        &mut self,
        image_data: &[u8],
        width: u32,
        height: u32,
    ) -> anyhow::Result<Vec<FaceDetectionResult>> {
        // 1. 预处理 (计算缩放比例和偏移量)
        let scale =
            (self.input_width as f32 / width as f32).min(self.input_height as f32 / height as f32);
        let pad_x = (self.input_width as f32 - width as f32 * scale) / 2.0;
        let pad_y = (self.input_height as f32 - height as f32 * scale) / 2.0;

        // 此处调用你之前的 preprocess_image_letterbox
        let input_data = preprocess_image_letterbox(
            image_data,
            width,
            height,
            self.input_width,
            self.input_height,
        )?;

        // 2. RKNN 推理
        let mut input = RknnInput {
            index: 0,
            buf: input_data,
            pass_through: false,
            type_: RknnTensorType::Uint8,
            fmt: RknnTensorFormat::NHWC,
        };
        self.rknn.input_set(&mut input)?;
        self.rknn.run()?;

        // 3. 获取输出 Tensor (loc, conf, landmarks)
        let rknn_output: RknnOutput<f32> = self.rknn.outputs_get()?;
        let output_slice = rknn_output.to_vec();

        // 4. 后处理 (兼容原有接口)
        Ok(postprocess_retinaface(
            &output_slice,
            &self.priors,
            width,
            height,
            self.input_width,
            self.input_height,
            scale,
            pad_x,
            pad_y,
            self.conf_threshold,
        ))
    }
}

/// 核心后处理：将 RetinaFace 的输出解码为标准结果
fn postprocess_retinaface(
    output: &[f32],
    priors: &[[f32; 4]],
    orig_w: u32,
    orig_h: u32,
    model_w: u32,
    model_h: u32,
    scale: f32,
    pad_x: f32,
    pad_y: f32,
    conf_thresh: f32,
) -> Vec<FaceDetectionResult> {
    let n = priors.len();
    let mut results = Vec::new();
    let variances = [0.1, 0.2];

    // RetinaFace 输出布局通常为:
    // [0..n*4] 是 loc, [n*4..n*6] 是 conf, [n*6..n*16] 是 landmarks
    let conf_base = n * 4;

    for i in 0..n {
        let score = output[conf_base + i * 2 + 1];
        if score > conf_thresh {
            let p = &priors[i];

            // 解码中心点和宽高
            let cx = p[0] + output[i * 4 + 0] * variances[0] * p[2];
            let cy = p[1] + output[i * 4 + 1] * variances[0] * p[3];
            let w = (output[i * 4 + 2] * variances[1]).exp() * p[2];
            let h = (output[i * 4 + 3] * variances[1]).exp() * p[3];

            // 转换回原始图像坐标系 (归一化 0~1)
            let x1 = (cx - w / 2.0) * model_w as f32;
            let y1 = (cy - h / 2.0) * model_h as f32;

            let real_x = (x1 - pad_x) / scale / orig_w as f32;
            let real_y = (y1 - pad_y) / scale / orig_h as f32;
            let real_w = (w * model_w as f32) / scale / orig_w as f32;
            let real_h = (h * model_h as f32) / scale / orig_h as f32;

            results.push(FaceDetectionResult {
                has_face: true,
                x: real_x.max(0.0),
                y: real_y.max(0.0),
                width: real_w,
                height: real_h,
                confidence: score,
                // 如果你的 FaceDetectionResult 结构体支持，可以在这里添加 landmarks
            });
        }
    }

    // 调用你原来的 NMS
    nms_filter(results, 0.45)
}

/// 生成 PriorBox 逻辑 (只需在初始化时运行一次)
fn generate_priors(mw: u32, mh: u32) -> Vec<[f32; 4]> {
    let mut priors = Vec::new();
    let steps = [8, 16, 32];
    let min_sizes = [vec![16, 32], vec![64, 128], vec![256, 512]];
    for (k, step) in steps.iter().enumerate() {
        let fw = (mw as f32 / *step as f32).ceil() as u32;
        let fh = (mh as f32 / *step as f32).ceil() as u32;
        for i in 0..fh {
            for j in 0..fw {
                for ms in &min_sizes[k] {
                    let sx = *ms as f32 / mw as f32;
                    let sy = *ms as f32 / mh as f32;
                    let cx = (j as f32 + 0.5) * *step as f32 / mw as f32;
                    let cy = (i as f32 + 0.5) * *step as f32 / mh as f32;
                    priors.push([cx, cy, sx, sy]);
                }
            }
        }
    }
    priors
}

/// 测试 RetinaFace 检测
pub fn test_retinaface(model_path: PathBuf, test_image_path: PathBuf) -> anyhow::Result<()> {
    use super::detector::draw_hollow_rect_static;

    log::info!("Testing RetinaFace RKNN detection");
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

    // 绘制结果
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
    let output_path = PathBuf::from("retinaface_test_result.png");
    result_img.save(&output_path)?;
    log::info!("Result saved to: {:?}", output_path);

    Ok(())
}
