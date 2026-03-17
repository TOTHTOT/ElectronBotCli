//! 人脸检测 trait 定义
use std::path::PathBuf;

/// 人脸检测结果
#[derive(Debug, Clone, Default)]
pub struct FaceDetectionResult {
    pub has_face: bool,
    pub x: f32,      // 归一化中心 X
    pub y: f32,      // 归一化中心 Y
    pub width: f32,  // 归一化宽度
    pub height: f32, // 归一化高度
    pub confidence: f32,
}

/// 人脸检测器 trait
pub trait FaceDetectorTrait: Send + Sync {
    /// 检测单个人脸（默认实现：返回检测到的第一个人脸）
    fn detect(
        &mut self,
        rgb_data: &[u8],
        width: u32,
        height: u32,
    ) -> anyhow::Result<FaceDetectionResult> {
        let start_time = std::time::Instant::now();
        let results = self.detect_multiple(rgb_data, width, height)?;
        log::debug!("detect used time: {:?}", start_time.elapsed());
        Ok(results.into_iter().next().unwrap_or_default())
    }

    /// 检测多个人脸
    fn detect_multiple(
        &mut self,
        image_data: &[u8],
        width: u32,
        height: u32,
    ) -> anyhow::Result<Vec<FaceDetectionResult>>;
}

/// 在 RGB 原始数据上画空心矩形框（静态函数）
#[allow(clippy::too_many_arguments)]
pub fn draw_hollow_rect_static(
    data: &mut [u8],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    color: [u8; 3],
) {
    let mut set_pixel = |px: u32, py: u32| {
        if px >= width || py >= height {
            return;
        }
        let idx = (py * width + px) as usize * 3;
        if idx + 2 < data.len() {
            data[idx..idx + 3].copy_from_slice(&color);
        }
    };

    // 画水平线（上下边）
    for i in 0..w {
        let curr_x = x + (i as i32);
        if curr_x >= 0 && curr_x < (width as i32) {
            set_pixel(curr_x as u32, y.max(0) as u32);
            let bottom = (y + (h as i32) - 1).max(0) as u32;
            if bottom < height {
                set_pixel(curr_x as u32, bottom);
            }
        }
    }

    // 画垂直线（左右边）
    for i in 0..h {
        let curr_y = y + (i as i32);
        if curr_y >= 0 && curr_y < (height as i32) {
            set_pixel(x.max(0) as u32, curr_y as u32);
            let right = (x + (w as i32) - 1).max(0) as u32;
            if right < width {
                set_pixel(right, curr_y as u32);
            }
        }
    }
}

/// 动态创建人脸检测器
/// 根据模型文件后缀自动选择后端
pub fn create_face_detector(model_path: PathBuf) -> anyhow::Result<Box<dyn FaceDetectorTrait>> {
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        log::info!("Creating RKNN face detector: {:?}", model_path);
        let detector = super::rknn::RknnFaceDetector::new(model_path)?;
        Ok(Box::new(detector))
    }
    #[cfg(not(all(target_os = "linux", target_arch = "aarch64")))]
    {
        log::info!("Creating ONNX face detector: {:?}", model_path);
        let detector = super::ort::OrtFaceDetector::new(model_path)?;
        Ok(Box::new(detector))
    }
}

/// NMS 去重
pub fn nms_filter(
    detections: Vec<FaceDetectionResult>,
    iou_threshold: f32,
) -> Vec<FaceDetectionResult> {
    if detections.is_empty() {
        return detections;
    }

    let mut keep = Vec::new();
    let mut suppressed = vec![false; detections.len()];

    for i in 0..detections.len() {
        if suppressed[i] {
            continue;
        }

        keep.push(detections[i].clone());

        for j in (i + 1)..detections.len() {
            if suppressed[j] {
                continue;
            }

            let iou = calculate_iou(&detections[i], &detections[j]);
            if iou > iou_threshold {
                suppressed[j] = true;
            }
        }
    }

    keep
}

/// 计算 IoU
pub fn calculate_iou(a: &FaceDetectionResult, b: &FaceDetectionResult) -> f32 {
    let a_x1 = a.x - a.width / 2.0;
    let a_y1 = a.y - a.height / 2.0;
    let a_x2 = a.x + a.width / 2.0;
    let a_y2 = a.y + a.height / 2.0;

    let b_x1 = b.x - b.width / 2.0;
    let b_y1 = b.y - b.height / 2.0;
    let b_x2 = b.x + b.width / 2.0;
    let b_y2 = b.y + b.height / 2.0;

    let inter_x1 = a_x1.max(b_x1);
    let inter_y1 = a_y1.max(b_y1);
    let inter_x2 = a_x2.min(b_x2);
    let inter_y2 = a_y2.min(b_y2);

    let inter_width = (inter_x2 - inter_x1).max(0.0);
    let inter_height = (inter_y2 - inter_y1).max(0.0);
    let inter_area = inter_width * inter_height;

    let a_area = a.width * a.height;
    let b_area = b.width * b.height;
    let union_area = a_area + b_area - inter_area;

    if union_area <= 0.0 {
        0.0
    } else {
        inter_area / union_area
    }
}
