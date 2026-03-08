//! YOLOv8 人脸检测模块 - 修复版

use image::{DynamicImage, Rgb, RgbImage};
use ort::session::{Session, SessionOutputs};
use ort::value::Tensor;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

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
    session: Arc<Mutex<Session>>,
    input_width: u32,
    input_height: u32,
    conf_threshold: f32,
}

impl FaceDetector {
    pub fn new(model_path: PathBuf) -> anyhow::Result<Self> {
        let session = Session::builder()?.commit_from_file(model_path)?;
        Ok(Self {
            session: Arc::new(Mutex::new(session)),
            input_width: 640,
            input_height: 640,
            conf_threshold: 0.35,
        })
    }

    pub fn detect(
        &self,
        image_data: &[u8],
        width: u32,
        height: u32,
    ) -> anyhow::Result<FaceDetectionResult> {
        let scale =
            (self.input_width as f32 / width as f32).min(self.input_height as f32 / height as f32);

        let input_tensor = self.preprocess(image_data, width, height)?;
        let mut session = self.session.lock().unwrap();
        let outputs = session.run(ort::inputs!["images" => input_tensor])?;

        // 传入原图宽高
        self.postprocess(outputs, scale, width, height)
    }

    fn preprocess(
        &self,
        image_data: &[u8],
        img_width: u32,
        img_height: u32,
    ) -> anyhow::Result<Tensor<f32>> {
        let (tw, th) = (self.input_width, self.input_height);

        // 修复：使用 .to_vec() 确保数据拥有所有权，从而匹配 RgbImage (ImageBuffer<Rgb<u8>, Vec<u8>>)
        let img_buffer = RgbImage::from_raw(img_width, img_height, image_data.to_vec())
            .ok_or_else(|| anyhow::anyhow!("Failed to create image buffer"))?;
        let dynamic_img = DynamicImage::ImageRgb8(img_buffer);

        // Letterbox 缩放
        let scale = (tw as f32 / img_width as f32).min(th as f32 / img_height as f32);
        let nw = (img_width as f32 * scale) as u32;
        let nh = (img_height as f32 * scale) as u32;

        // 修复：确保 resize 后的图像转回 Rgb8，以匹配 canvas 的像素类型
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
        &self,
        outputs: SessionOutputs<'_>,
        scale: f32,
        img_width: u32,
        img_height: u32,
    ) -> anyhow::Result<FaceDetectionResult> {
        let output = &outputs[0];
        let arr = output.try_extract_array::<f32>()?;
        let view = arr.view();
        let slice = view
            .as_slice()
            .ok_or_else(|| anyhow::anyhow!("Failed to get slice"))?;

        let num_anchors = 8400;
        let mut best_score = 0.0f32;
        let mut best_data_640 = [0.0f32; 4];

        for i in 0..num_anchors {
            let score = slice[4 * num_anchors + i];
            if score > self.conf_threshold && score > best_score {
                best_score = score;
                best_data_640 = [
                    slice[i],
                    slice[num_anchors + i],
                    slice[2 * num_anchors + i],
                    slice[3 * num_anchors + i],
                ];
            }
        }

        if best_score > 0.0 {
            // 1. 将 640 空间的像素坐标还原回原图空间的绝对像素值 (对标 Python: box / scale)
            let x_orig_px = best_data_640[0] / scale;
            let y_orig_px = best_data_640[1] / scale;
            let w_orig_px = best_data_640[2] / scale;
            let h_orig_px = best_data_640[3] / scale;

            // 2. 将绝对像素值转为归一化比例 (0.0 ~ 1.0)
            let norm_x = x_orig_px / img_width as f32;
            let norm_y = y_orig_px / img_height as f32;
            let norm_w = w_orig_px / img_width as f32;
            let norm_h = h_orig_px / img_height as f32;

            log::info!(
                "Final Normalized Box: [x={:.4}, y={:.4}, w={:.4}, h={:.4}]",
                norm_x,
                norm_y,
                norm_w,
                norm_h
            );

            Ok(FaceDetectionResult {
                has_face: true,
                x: norm_x,
                y: norm_y,
                width: norm_w,
                height: norm_h,
                confidence: best_score,
            })
        } else {
            Ok(FaceDetectionResult::default())
        }
    }
}

// --- 单元测试部分 ---
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_face_detection() -> anyhow::Result<()> {
        let model_path = PathBuf::from("/Users/yangyihui/.cache/huggingface/hub/models--deepghs--yolo-face/snapshots/e3662574830c534dfcc9c3b7ea4d89272f8aae4e/yolov8n-face/model.onnx"); // 确保路径正确
        let detector = FaceDetector::new(model_path)?;

        let img_path = "assets/images/figure2.png";
        let img = image::open(img_path)?.to_rgb8();
        let (w, h) = img.dimensions();

        let result = detector.detect(&img.clone().into_raw(), w, h)?;

        if result.has_face {
            println!("Face detected! Confidence: {:.3}", result.confidence);

            let mut out_img = img;
            let x1 = (result.x - result.width / 2.0) * w as f32;
            let y1 = (result.y - result.height / 2.0) * h as f32;
            let bw = result.width * w as f32;
            let bh = result.height * h as f32;
            println!("rect: [{},{},{},{}]", x1, y1, bw, bh);
            draw_hollow_rect(
                &mut out_img,
                x1 as i32,
                y1 as i32,
                bw as u32,
                bh as u32,
                Rgb([255, 0, 0]),
            );
            out_img.save("assets/images/rust_result.png")?;
            println!("Result saved to assets/images/rust_result.png");
        } else {
            println!("No face detected.");
        }
        Ok(())
    }

    fn draw_hollow_rect(img: &mut RgbImage, x: i32, y: i32, w: u32, h: u32, color: Rgb<u8>) {
        // 修复：通过显式括号解决 < 运算符优先级歧义
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
