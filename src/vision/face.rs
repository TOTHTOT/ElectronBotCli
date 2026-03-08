//! YOLOv8 人脸检测模块

use ort::session::{Session, SessionOutputs};
use ort::value::Tensor;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// 人脸检测结果
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

/// YOLOv8 人脸检测器
pub struct FaceDetector {
    session: Arc<Mutex<Session>>,
    input_width: i32,
    input_height: i32,
    conf_threshold: f32,
}

impl FaceDetector {
    /// 创建新的检测器
    pub fn new(model_path: PathBuf) -> anyhow::Result<Self> {
        log::info!("Loading YOLOv8 face detector from: {:?}", model_path);
        log::info!("ONNX Runtime may download binaries on first run, this can take a while...");

        let start = std::time::Instant::now();
        let session = Session::builder()?.commit_from_file(model_path)?;

        log::info!(
            "YOLOv8 face detector loaded successfully in {:?}",
            start.elapsed()
        );

        Ok(Self {
            session: Arc::new(Mutex::new(session)),
            input_width: 640,
            input_height: 640,
            conf_threshold: 0.5,
        })
    }

    /// 检测人脸
    pub fn detect(
        &self,
        image_data: &[u8],
        width: u32,
        height: u32,
    ) -> anyhow::Result<FaceDetectionResult> {
        // 预处理图像
        let input_tensor = self.preprocess(image_data, width, height)?;

        // 执行推理
        let mut session = self.session.lock().unwrap();
        let outputs = session.run(ort::inputs!["images" => input_tensor])?;

        // 后处理
        self.postprocess(outputs)
    }

    /// 预处理图像
    fn preprocess(
        &self,
        image_data: &[u8],
        img_width: u32,
        img_height: u32,
    ) -> anyhow::Result<Tensor<f32>> {
        let (w, h) = (self.input_width, self.input_height);

        // 创建 RGB 格式的输入 (1, 3, 640, 640)
        let mut input: Vec<f32> = vec![0.0f32; (3 * w * h) as usize];

        // 读取 BGR 数据并 resize
        let scale_x = w as f32 / img_width as f32;
        let scale_y = h as f32 / img_height as f32;

        for y in 0..img_height {
            for x in 0..img_width {
                let src_idx = ((y * img_width + x) * 3) as usize;
                if src_idx + 2 >= image_data.len() {
                    continue;
                }

                // BGR 转 RGB，并进行 normalization
                let dst_x = (x as f32 * scale_x) as i32;
                let dst_y = (y as f32 * scale_y) as i32;

                if dst_x >= 0 && dst_x < w && dst_y >= 0 && dst_y < h {
                    // BGR format -> RGB
                    let b = image_data[src_idx] as f32 / 255.0;
                    let g = image_data[src_idx + 1] as f32 / 255.0;
                    let r = image_data[src_idx + 2] as f32 / 255.0;

                    // Channel first: (C, H, W)
                    let r_idx = (dst_y * w + dst_x) as usize;
                    let g_idx = (w * h + dst_y * w + dst_x) as usize;
                    let b_idx = (2 * w * h + dst_y * w + dst_x) as usize;

                    input[r_idx] = r;
                    input[g_idx] = g;
                    input[b_idx] = b;
                }
            }
        }

        // 创建 Tensor，使用 (data, shape) 元组
        let shape: (Vec<i32>, Vec<f32>) = (vec![1, 3, h, w], input);
        let tensor = Tensor::from_array(shape)?;
        Ok(tensor)
    }

    /// 后处理 - 解析 YOLOv8 输出
    fn postprocess(&self, outputs: SessionOutputs<'_>) -> anyhow::Result<FaceDetectionResult> {
        // 获取输出
        let output = &outputs[0];
        let arr = output.try_extract_array::<f32>()?;

        // 使用 as_slice() 返回 Option，需要 unwrap
        let arr_slice = match arr.as_slice() {
            Some(slice) => slice,
            None => {
                log::warn!("Failed to extract output slice");
                return Ok(FaceDetectionResult::default());
            }
        };

        if arr_slice.is_empty() {
            return Ok(FaceDetectionResult::default());
        }

        // 假设输出形状为 [1, num_boxes, 85]
        // 85 = 4 (x, y, w, h) + 1 (conf) + 80 (classes)
        let features = 85;
        let total_len = arr_slice.len();
        let num_boxes = total_len / features;

        log::debug!("YOLOv8 output: {} boxes, {} features", num_boxes, features);

        // 查找最大置信度的人脸
        let mut best_conf = 0.0f32;
        let mut best_box = [0.0f32; 4];

        for i in 0..num_boxes {
            let base = i * features;

            if base + 4 >= total_len {
                break;
            }

            // 格式: [x, y, w, h, conf, class0, class1, ...]
            let conf = arr_slice[base + 4];

            if conf > self.conf_threshold && conf > best_conf {
                best_conf = conf;
                best_box = [
                    arr_slice[base],
                    arr_slice[base + 1],
                    arr_slice[base + 2],
                    arr_slice[base + 3],
                ];
            }
        }

        if best_conf > 0.0 {
            // 转换到归一化坐标 (cx, cy, w, h)
            let cx = best_box[0] / self.input_width as f32;
            let cy = best_box[1] / self.input_height as f32;
            let w = best_box[2] / self.input_width as f32;
            let h = best_box[3] / self.input_height as f32;

            log::info!(
                "Face detected: conf={:.3}, bbox=[{:.2}, {:.2}, {:.2}, {:.2}]",
                best_conf,
                cx,
                cy,
                w,
                h
            );

            Ok(FaceDetectionResult {
                has_face: true,
                x: cx,
                y: cy,
                width: w,
                height: h,
                confidence: best_conf,
            })
        } else {
            Ok(FaceDetectionResult::default())
        }
    }
}
