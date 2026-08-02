//! RetinaFace RKNN 人脸检测后端
use super::detector::{nms_filter, FaceDetectionResult, FaceDetectorTrait};
use rknn_rs::prelude::{Rknn, RknnInput, RknnTensorFormat, RknnTensorType};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Instant;

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
use crate::media::video::rga_adapter::RgaHelper;

// RGA 辅助单例 (仅在 aarch64 Linux 上存在)
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
static RGA_HELPER: OnceLock<RgaHelper> = OnceLock::new();

pub struct RknnFaceDetector {
    rknn: Rknn,
    input_width: u32,
    input_height: u32,
    conf_threshold: f32,
    priors: Vec<[f32; 4]>,
}

impl RknnFaceDetector {
    pub fn new(model_path: PathBuf) -> anyhow::Result<Self> {
        let rknn = Rknn::new(&model_path)?;
        let input_width = 320;
        let input_height = 320;
        let priors = generate_priors(input_width, input_height);

        Ok(Self {
            rknn,
            input_width,
            input_height,
            conf_threshold: 0.3,
            priors,
        })
    }
}

/// Letterbox 预处理: 缩放 + BGR->RGB
fn preprocess_image(
    image_data: &[u8],
    src_width: u32,
    src_height: u32,
    dst_width: u32,
    dst_height: u32,
) -> Vec<u8> {
    // 尝试使用 RGA 硬件加速
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        let helper = RGA_HELPER.get_or_init(RgaHelper::new);
        // RGA resize 零拷贝借用原帧, 失败时返回空数据 (实际不会走到)
        let result = helper
            .resize(image_data, src_width, src_height, dst_width, dst_height)
            .unwrap_or_else(|| vec![0u8; (dst_width * dst_height * 3) as usize]);
        return result;
    }

    #[cfg(not(all(target_os = "linux", target_arch = "aarch64")))]
    {
        let image_ref: &[u8] = image_data;
        let mut canvas = vec![0u8; (dst_width * dst_height * 3) as usize];

        let scale =
            (dst_width as f32 / src_width as f32).min(dst_height as f32 / src_height as f32);
        let new_width = (src_width as f32 * scale) as u32;
        let new_height = (src_height as f32 * scale) as u32;
        let x_offset = (dst_width - new_width) / 2;
        let y_offset = (dst_height - new_height) / 2;

        for dy in 0..new_height {
            for dx in 0..new_width {
                let sx = (dx as f32 / scale) as u32;
                let sy = (dy as f32 / scale) as u32;
                let src_idx = ((sy * src_width + sx) * 3) as usize;
                let dst_idx = (((dy + y_offset) * dst_width + dx + x_offset) * 3) as usize;

                if src_idx + 2 < image_ref.len() && dst_idx + 2 < canvas.len() {
                    canvas[dst_idx + 0] = image_ref[src_idx + 2]; // B -> R
                    canvas[dst_idx + 1] = image_ref[src_idx + 1];
                    canvas[dst_idx + 2] = image_ref[src_idx + 0]; // R -> B
                }
            }
        }
        canvas
    }
}

impl FaceDetectorTrait for RknnFaceDetector {
    fn detect_multiple(
        &mut self,
        rgb_data: &[u8],
        width: u32,
        height: u32,
    ) -> anyhow::Result<Vec<FaceDetectionResult>> {
        let total_start = Instant::now();

        let preprocess_start = Instant::now();
        let scale =
            (self.input_width as f32 / width as f32).min(self.input_height as f32 / height as f32);
        let pad_x = (self.input_width as f32 - width as f32 * scale) / 2.0;
        let pad_y = (self.input_height as f32 - height as f32 * scale) / 2.0;
        // 输入已是模型尺寸时跳过缩放: capture 管线在检测帧会直接喂
        // "YUYV -> CSC+旋转+缩到 320x320" 单硬件 pass 的产物 (~1.2ms),
        // 替代这里的全帧图再 resize (~2.9ms). 此时 scale=1/pad=0,
        // 后处理坐标映射天然一致.
        let input_data = if width == self.input_width && height == self.input_height {
            rgb_data.to_vec()
        } else {
            preprocess_image(rgb_data, width, height, self.input_width, self.input_height)
        };
        let preprocess_time = preprocess_start.elapsed();

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

        let output_start = Instant::now();
        let loc_output = self.rknn.outputs_get_by_index::<f32>(0, true)?;
        let conf_output = self.rknn.outputs_get_by_index::<f32>(1, true)?;
        let _landmarks_output = self.rknn.outputs_get_by_index::<f32>(2, true)?;
        let loc = loc_output.to_vec();
        let conf = conf_output.to_vec();
        let output_time = output_start.elapsed();

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
        log::debug!(
            "RKNN: preprocess={:.1}ms, inference={:.1}ms, output={:.1}ms, postprocess={:.1}ms, total={:.1}ms",
            preprocess_time.as_secs_f64() * 1000.0,
            inference_time.as_secs_f64() * 1000.0,
            output_time.as_secs_f64() * 1000.0,
            postprocess_time.as_secs_f64() * 1000.0,
            total_time.as_secs_f64() * 1000.0
        );

        Ok(results)
    }

    fn input_size(&self) -> Option<(u32, u32)> {
        Some((self.input_width, self.input_height))
    }
}

/// 后处理: 解码 + NMS
fn postprocess_retinaface(
    loc: &[f32],
    conf: &[f32],
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

    for i in 0..n {
        let score = conf[i * 2 + 1];
        if score > conf_thresh {
            let p = &priors[i];
            let cx = p[0] + loc[i * 4 + 0] * variances[0] * p[2];
            let cy = p[1] + loc[i * 4 + 1] * variances[0] * p[3];
            let w = (loc[i * 4 + 2] * variances[1]).exp() * p[2];
            let h = (loc[i * 4 + 3] * variances[1]).exp() * p[3];

            let model_x = cx * model_w as f32;
            let model_y = cy * model_h as f32;
            let model_box_w = w * model_w as f32;
            let model_box_h = h * model_h as f32;

            let real_x_px = (model_x - pad_x) / scale;
            let real_y_px = (model_y - pad_y) / scale;
            let real_w_px = model_box_w / scale;
            let real_h_px = model_box_h / scale;

            results.push(FaceDetectionResult {
                has_face: true,
                x: real_x_px / orig_w as f32, // 中心点 X
                y: real_y_px / orig_h as f32, // 中心点 Y
                width: real_w_px / orig_w as f32,
                height: real_h_px / orig_h as f32,
                confidence: score,
            });
        }
    }

    let results = nms_filter(results, 0.45);
    results.into_iter().filter(|r| r.confidence > 0.2).collect()
}

/// 生成 PriorBox
fn generate_priors(mw: u32, mh: u32) -> Vec<[f32; 4]> {
    let mut priors = Vec::new();
    let steps = [8, 16, 32];
    let min_sizes = [[16, 32], [64, 128], [256, 512]];
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

    let img = image::open(&test_image_path)?;
    let rgb_img = img.to_rgb8();
    let (width, height) = rgb_img.dimensions();

    let mut detector = RknnFaceDetector::new(model_path)?;

    let rgb_data = rgb_img.as_raw().to_vec();
    test_detector_speed(width, height, &mut detector, &rgb_data)?;
    let results = detector.detect_multiple(&rgb_data, width, height)?;

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

    result_img.save("retinaface_test_result.png")?;
    Ok(())
}

fn test_detector_speed(
    width: u32,
    height: u32,
    detector: &mut RknnFaceDetector,
    rgb_data: &Vec<u8>,
) -> anyhow::Result<()> {
    let mut total_sum: f64 = 0.0;
    let iterations = 10;
    for i in 0..iterations {
        let start = Instant::now();
        let results = detector.detect_multiple(rgb_data, width, height)?;
        let total = start.elapsed();

        if i == iterations - 1 {
            log::info!("Last detection result: {} faces detected", results.len());
        }

        total_sum += total.as_secs_f64() * 1000.0;
    }

    let avg_total = total_sum / iterations as f64;
    log::info!(
        "Average time ({} iterations): total={:.1}ms",
        iterations,
        avg_total
    );
    Ok(())
}
