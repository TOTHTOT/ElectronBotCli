//! YOLOv8 人脸检测模块 - 修复版

use image::{DynamicImage, Rgb, RgbImage};
use ort::session::{Session, SessionOutputs};
use ort::value::Tensor;
use std::path::PathBuf;

#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct FaceDetectionResult {
    pub has_face: bool,
    pub x: f32,      // 归一化中心 X
    pub y: f32,      // 归一化中心 Y
    pub width: f32,  // 归一化宽度
    pub height: f32, // 归一化高度
    pub confidence: f32,
}

pub struct FaceDetector {
    session: Session,
    input_width: u32,
    input_height: u32,
    conf_threshold: f32,
}

impl FaceDetector {
    pub fn new(model_path: PathBuf) -> anyhow::Result<Self> {
        let session = Session::builder()?.commit_from_file(model_path)?;
        Ok(Self {
            session,
            input_width: 640,
            input_height: 640,
            conf_threshold: 0.35,
        })
    }

    pub fn detect(
        &mut self,
        image_data: &[u8],
        width: u32,
        height: u32,
    ) -> anyhow::Result<FaceDetectionResult> {
        let scale =
            (self.input_width as f32 / width as f32).min(self.input_height as f32 / height as f32);

        let input_tensor = self.preprocess(image_data, width, height)?;
        let outputs = self.session.run(ort::inputs!["images" => input_tensor])?;

        // 传入原图宽高
        Self::postprocess(outputs, scale, width, height, self.conf_threshold)
    }

    /// 检测多张人脸，返回所有结果
    /// 按置信度降序排序
    /// # Arguments
    ///
    /// * `image_data`:
    /// * `width`:
    /// * `height`:
    ///
    /// returns: Result<Vec<FaceDetectionResult, Global>, Error>
    ///
    /// # Examples
    ///
    /// ```
    ///
    /// ```
    #[cfg(test)]
    pub fn detect_multiple(
        &mut self,
        image_data: &[u8],
        width: u32,
        height: u32,
    ) -> anyhow::Result<Vec<FaceDetectionResult>> {
        let scale =
            (self.input_width as f32 / width as f32).min(self.input_height as f32 / height as f32);

        let input_tensor = self.preprocess(image_data, width, height)?;
        let outputs = self.session.run(ort::inputs!["images" => input_tensor])?;

        Self::postprocess_multiple(outputs, scale, width, height, self.conf_threshold)
    }

    fn preprocess(
        &self,
        image_data: &[u8],
        img_width: u32,
        img_height: u32,
    ) -> anyhow::Result<Tensor<f32>> {
        let (tw, th) = (self.input_width, self.input_height);

        let img_buffer = RgbImage::from_raw(img_width, img_height, image_data.to_vec())
            .ok_or_else(|| anyhow::anyhow!("Failed to create image buffer"))?;
        let dynamic_img = DynamicImage::ImageRgb8(img_buffer);

        let scale = (tw as f32 / img_width as f32).min(th as f32 / img_height as f32);
        let nw = (img_width as f32 * scale) as u32;
        let nh = (img_height as f32 * scale) as u32;

        let resized = dynamic_img
            .resize_exact(nw, nh, image::imageops::FilterType::Triangle)
            .to_rgb8();

        let mut canvas = RgbImage::from_pixel(tw, th, Rgb([114, 114, 114]));
        image::imageops::overlay(&mut canvas, &resized, 0, 0);

        let mut input = vec![0.0f32; (3 * tw * th) as usize];
        let area = (tw * th) as usize;
        for (i, pixel) in canvas.pixels().enumerate() {
            input[i] = pixel.0[0] as f32 / 255.0;
            input[i + area] = pixel.0[1] as f32 / 255.0;
            input[i + 2 * area] = pixel.0[2] as f32 / 255.0;
        }

        let tensor = Tensor::from_array((vec![1, 3, th as i32, tw as i32], input))?;
        Ok(tensor)
    }
    fn postprocess(
        outputs: SessionOutputs<'_>,
        scale: f32,
        img_width: u32,
        img_height: u32,
        conf_threshold: f32,
    ) -> anyhow::Result<FaceDetectionResult> {
        let results =
            Self::postprocess_multiple(outputs, scale, img_width, img_height, conf_threshold)?;
        Ok(results.into_iter().next().unwrap_or_default())
    }

    fn postprocess_multiple(
        outputs: SessionOutputs<'_>,
        scale: f32,
        img_width: u32,
        img_height: u32,
        conf_threshold: f32,
    ) -> anyhow::Result<Vec<FaceDetectionResult>> {
        let output = &outputs[0];
        let arr = output.try_extract_array::<f32>()?;
        let view = arr.view();
        let slice = view
            .as_slice()
            .ok_or_else(|| anyhow::anyhow!("Failed to get slice"))?;

        let num_anchors = 8400;
        let mut results = Vec::new();

        for i in 0..num_anchors {
            let score = slice[4 * num_anchors + i];
            if score > conf_threshold {
                let box_data = [
                    slice[i],
                    slice[num_anchors + i],
                    slice[2 * num_anchors + i],
                    slice[3 * num_anchors + i],
                ];

                // 转换坐标到原图空间
                let x_orig_px = box_data[0] / scale;
                let y_orig_px = box_data[1] / scale;
                let w_orig_px = box_data[2] / scale;
                let h_orig_px = box_data[3] / scale;

                // 归一化
                let norm_x = x_orig_px / img_width as f32;
                let norm_y = y_orig_px / img_height as f32;
                let norm_w = w_orig_px / img_width as f32;
                let norm_h = h_orig_px / img_height as f32;

                results.push(FaceDetectionResult {
                    has_face: true,
                    x: norm_x,
                    y: norm_y,
                    width: norm_w,
                    height: norm_h,
                    confidence: score,
                });
            }
        }

        // 按置信度降序排序
        results.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());

        // NMS 去重
        let results = Self::nms_filter(results, 0.5);

        log::info!("Detected {} faces (after NMS)", results.len());
        for (i, r) in results.iter().enumerate() {
            log::info!(
                "  Face {}: conf={:.3}, box=[{:.3}, {:.3}, {:.3}, {:.3}]",
                i,
                r.confidence,
                r.x,
                r.y,
                r.width,
                r.height
            );
        }

        Ok(results)
    }

    /// NMS去重
    ///
    /// # Arguments
    ///
    /// * `detections`: 检测结果列表（需已按置信度降序排序）
    /// * `iou_threshold`: IoU 阈值，高于此值的框会被移除
    ///
    /// returns: 过滤后的结果
    fn nms_filter(
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

                let iou = Self::calculate_iou(&detections[i], &detections[j]);
                if iou > iou_threshold {
                    suppressed[j] = true;
                }
            }
        }

        keep
    }

    /// 计算两个检测框的 IoU
    fn calculate_iou(a: &FaceDetectionResult, b: &FaceDetectionResult) -> f32 {
        // 转换为左上角右下角坐标
        let a_x1 = a.x - a.width / 2.0;
        let a_y1 = a.y - a.height / 2.0;
        let a_x2 = a.x + a.width / 2.0;
        let a_y2 = a.y + a.height / 2.0;

        let b_x1 = b.x - b.width / 2.0;
        let b_y1 = b.y - b.height / 2.0;
        let b_x2 = b.x + b.width / 2.0;
        let b_y2 = b.y + b.height / 2.0;

        // 计算交集
        let inter_x1 = a_x1.max(b_x1);
        let inter_y1 = a_y1.max(b_y1);
        let inter_x2 = a_x2.min(b_x2);
        let inter_y2 = a_y2.min(b_y2);

        let inter_width = (inter_x2 - inter_x1).max(0.0);
        let inter_height = (inter_y2 - inter_y1).max(0.0);
        let inter_area = inter_width * inter_height;

        // 计算并集
        let a_area = a.width * a.height;
        let b_area = b.width * b.height;
        let union_area = a_area + b_area - inter_area;

        if union_area <= 0.0 {
            0.0
        } else {
            inter_area / union_area
        }
    }

    /// 在 RGB 原始数据上画空心矩形框
    ///
    /// # Arguments
    ///
    /// * `data`: RGB 字节数据
    /// * `width`: 图像宽度
    /// * `height`: 图像高度
    /// * `x`: 左上角 x 坐标
    /// * `y`: 左上角 y 坐标
    /// * `w`: 框宽度
    /// * `h`: 框高度
    /// * `color`: RGB 颜色值 [R, G, B]
    #[allow(clippy::too_many_arguments)]
    pub fn draw_hollow_rect_on_raw(
        data: &mut [u8],
        width: u32,
        height: u32,
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        color: [u8; 3],
    ) {
        // 辅助函数：设置像素
        let mut set_pixel = |px: u32, py: u32| {
            if px >= width || py >= height {
                return;
            }
            let idx = (py * width + px) as usize * 3;
            if idx + 2 < data.len() {
                data[idx..idx + 3].copy_from_slice(&color);
            }
        };

        // 绘制水平线（上下边框）
        for i in 0..w {
            let curr_x = x + (i as i32);
            if curr_x >= 0 && curr_x < (width as i32) {
                set_pixel(curr_x as u32, y as u32);
                let bottom = y + (h as i32) - 1;
                if bottom >= 0 && bottom < (height as i32) {
                    set_pixel(curr_x as u32, bottom as u32);
                }
            }
        }

        // 绘制垂直线（左右边框）
        for i in 0..h {
            let curr_y = y + (i as i32);
            if curr_y >= 0 && curr_y < (height as i32) {
                set_pixel(x as u32, curr_y as u32);
                let right = x + (w as i32) - 1;
                if right >= 0 && right < (width as i32) {
                    set_pixel(right as u32, curr_y as u32);
                }
            }
        }
    }

    /// 在图片上画框（用于测试，操作 RgbImage）
    ///
    /// # Arguments
    ///
    /// * `img`:
    /// * `x`:
    /// * `y`:
    /// * `w`:
    /// * `h`:
    /// * `color`:
    ///
    /// returns: ()
    ///
    /// # Examples
    ///
    /// ```
    ///
    /// ```
    #[cfg(test)]
    pub fn draw_hollow_rect(img: &mut RgbImage, x: i32, y: i32, w: u32, h: u32, color: Rgb<u8>) {
        for i in 0..w {
            let curr_x = x + (i as i32);
            if curr_x >= 0 && curr_x < (img.width() as i32) {
                if y >= 0 && y < (img.height() as i32) {
                    img.put_pixel(curr_x as u32, y as u32, color);
                }
                let bottom = y + (h as i32);
                if bottom >= 0 && bottom < (img.height() as i32) {
                    img.put_pixel(curr_x as u32, bottom as u32, color);
                }
            }
        }
        for i in 0..h {
            let curr_y = y + (i as i32);
            if curr_y >= 0 && curr_y < (img.height() as i32) {
                if x >= 0 && x < (img.width() as i32) {
                    img.put_pixel(x as u32, curr_y as u32, color);
                }
                let right = x + (w as i32);
                if right >= 0 && right < (img.width() as i32) {
                    img.put_pixel(right as u32, curr_y as u32, color);
                }
            }
        }
    }
}

// --- 单元测试部分 ---
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_manager::ModelManager;
    #[test]
    fn test_face_detection() -> anyhow::Result<()> {
        let mm = ModelManager::init()?;
        let mut detector = FaceDetector::new(mm.get("yolo_face").take().unwrap())?;

        let img_path = "assets/images/figure1.png";
        let img = image::open(img_path)?.to_rgb8();
        let (w, h) = img.dimensions();

        // 使用 detect_multiple 获取多张脸
        let results = detector.detect_multiple(&img.clone().into_raw(), w, h)?;

        if results.is_empty() {
            println!("No face detected.");
        } else {
            println!("Detected {} face(s)", results.len());

            let mut out_img = img;
            let colors = [
                Rgb([255, 0, 0]),   // 红色
                Rgb([0, 255, 0]),   // 绿色
                Rgb([0, 0, 255]),   // 蓝色
                Rgb([255, 255, 0]), // 黄色
                Rgb([255, 0, 255]), // 紫色
                Rgb([0, 255, 255]), // 青色
            ];

            for (i, result) in results.iter().enumerate() {
                println!(
                    "Face {}: confidence={:.3}, center=({:.3}, {:.3}), size={:.3}x{:.3}",
                    i + 1,
                    result.confidence,
                    result.x,
                    result.y,
                    result.width,
                    result.height
                );

                let x1 = (result.x - result.width / 2.0) * w as f32;
                let y1 = (result.y - result.height / 2.0) * h as f32;
                let bw = result.width * w as f32;
                let bh = result.height * h as f32;
                println!("  rect: [{:.1}, {:.1}, {:.1}, {:.1}]", x1, y1, bw, bh);

                let color = colors[i % colors.len()];
                FaceDetector::draw_hollow_rect(
                    &mut out_img,
                    x1 as i32,
                    y1 as i32,
                    bw as u32,
                    bh as u32,
                    color,
                );
            }

            out_img.save("assets/images/rust_result.png")?;
            println!("Result saved to assets/images/rust_result.png");
        }
        Ok(())
    }
}
