//! 兼容原有接口的 RetinaFace 增强后端
use super::detector::{nms_filter, FaceDetectionResult, FaceDetectorTrait};
use rknn_rs::prelude::{Rknn, RknnInput, RknnTensorFormat, RknnTensorType};
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
/// 预处理：letterbox resize + BGR->RGB（与Python的letterbox_resize + BGR2RGB一致）
fn preprocess_image_letterbox(
    image_data: &[u8],
    src_width: u32,
    src_height: u32,
    dst_width: u32,
    dst_height: u32,
) -> Vec<u8> {
    let mut canvas = vec![0u8; (dst_width * dst_height * 3) as usize];

    // 计算缩放
    let scale = (dst_width as f32 / src_width as f32).min(dst_height as f32 / src_height as f32);
    let new_width = (src_width as f32 * scale) as u32;
    let new_height = (src_height as f32 * scale) as u32;
    let x_offset = (dst_width - new_width) / 2;
    let y_offset = (dst_height - new_height) / 2;

    // 简单的最近邻缩放 + BGR->RGB
    for dy in 0..new_height {
        for dx in 0..new_width {
            let sx = (dx as f32 / scale) as u32;
            let sy = (dy as f32 / scale) as u32;
            let src_idx = ((sy * src_width + sx) * 3) as usize;
            let dst_idx = (((dy + y_offset) * dst_width + dx + x_offset) * 3) as usize;

            if src_idx + 2 < image_data.len() && dst_idx + 2 < canvas.len() {
                canvas[dst_idx + 0] = image_data[src_idx + 2]; // B -> R
                canvas[dst_idx + 1] = image_data[src_idx + 1]; // G
                canvas[dst_idx + 2] = image_data[src_idx + 0]; // R -> B
            }
        }
    }
    canvas
}
impl FaceDetectorTrait for RknnFaceDetector {
    fn detect_multiple(
        &mut self,
        image_data: &[u8],
        width: u32,
        height: u32,
    ) -> anyhow::Result<Vec<FaceDetectionResult>> {
        let total_start = Instant::now();

        // 1. 预处理: letterbox resize + BGR->RGB
        let preprocess_start = Instant::now();
        let scale =
            (self.input_width as f32 / width as f32).min(self.input_height as f32 / height as f32);
        let pad_x = (self.input_width as f32 - width as f32 * scale) / 2.0;
        let pad_y = (self.input_height as f32 - height as f32 * scale) / 2.0;

        let input_data = preprocess_image_letterbox(
            image_data,
            width,
            height,
            self.input_width,
            self.input_height,
        );
        let preprocess_time = preprocess_start.elapsed();

        // 2. RKNN 推理
        let inference_start = Instant::now();
        let mut input = RknnInput {
            index: 0,
            buf: input_data,
            pass_through: false,
            type_: RknnTensorType::Uint8,
            fmt: RknnTensorFormat::NHWC,
        };
        self.rknn.input_set(&mut input)?;
        self.rknn.run()?;
        let inference_time = inference_start.elapsed();

        // 3. 获取输出 Tensor (loc, conf, landmarks)
        let output_start = Instant::now();
        let loc_output = self.rknn.outputs_get_by_index::<f32>(0, true)?;
        let conf_output = self.rknn.outputs_get_by_index::<f32>(1, true)?;
        let landmarks_output = self.rknn.outputs_get_by_index::<f32>(2, true)?;
        let loc = loc_output.to_vec();
        let conf = conf_output.to_vec();
        let _landmarks = landmarks_output.to_vec();
        let output_time = output_start.elapsed();

        // 4. 后处理
        let postprocess_start = Instant::now();
        let results = postprocess_retinaface(
            &loc,
            &conf,
            &self.priors,
            width,
            height,
            self.input_width,
            self.input_height,
            scale,
            pad_x,
            pad_y,
            self.conf_threshold,
        );
        let postprocess_time = postprocess_start.elapsed();

        let total_time = total_start.elapsed();
        log::info!(
            "RKNN face detection: preprocess={:.1}ms, inference={:.1}ms, output={:.1}ms, postprocess={:.1}ms, total={:.1}ms",
            preprocess_time.as_secs_f64() * 1000.0,
            inference_time.as_secs_f64() * 1000.0,
            output_time.as_secs_f64() * 1000.0,
            postprocess_time.as_secs_f64() * 1000.0,
            total_time.as_secs_f64() * 1000.0
        );

        Ok(results)
    }
}

/// 核心后处理：将 RetinaFace 的输出解码为标准结果
fn postprocess_retinaface(
    loc: &[f32],      // [num_priors, 4]
    conf: &[f32],     // [num_priors, 2]
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

    log::info!("postprocess_retinaface: n={}, loc_len={}, conf_len={}", n, loc.len(), conf.len());

    // 检查长度
    if loc.len() < n * 4 || conf.len() < n * 2 {
        log::error!("Output too short! loc: {}, conf: {}, expected: {}", loc.len(), conf.len(), n * 4);
        return results;
    }

    for i in 0..n {
        // conf: [batch, num_priors, 2] -> flat: [priors*2 + class]
        // class 0 = background, class 1 = face
        let score = conf[i * 2 + 1]; // 人脸分数
        if score > conf_thresh {
            let p = &priors[i];

            // 解码中心点和宽高 (和Python代码一致)
            let cx = p[0] + loc[i * 4 + 0] * variances[0] * p[2];
            let cy = p[1] + loc[i * 4 + 1] * variances[0] * p[3];
            let w = (loc[i * 4 + 2] * variances[1]).exp() * p[2];
            let h = (loc[i * 4 + 3] * variances[1]).exp() * p[3];

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
            });
        }
    }

    log::info!("Found {} faces before NMS", results.len());

    // 调用你原来的 NMS
    let results = nms_filter(results, 0.45);
    log::info!("Found {} faces after NMS", results.len());

    // Python 代码中 NMS 之后还用了 0.2 阈值过滤最终结果
    results.into_iter().filter(|r| r.confidence > 0.2).collect()
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
    log::info!("Detector initialized");
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
