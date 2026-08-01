//! ONNX 人脸检测后端
#![allow(dead_code)]

use super::detector::{nms_filter, FaceDetectionResult, FaceDetectorTrait};
use image::{DynamicImage, Rgb, RgbImage};
use std::path::PathBuf;

pub struct OrtFaceDetector {
    session: ort::session::Session,
    input_width: u32,
    input_height: u32,
    conf_threshold: f32,
}

impl OrtFaceDetector {
    fn get_onnx_running_time_path() -> PathBuf {
        if cfg!(target_os = "macos") {
            PathBuf::from("/opt/homebrew/opt/onnxruntime/lib/libonnxruntime.dylib")
        } else if cfg!(target_os = "windows") {
            // 优先 exe 同目录, 再回退到 PATH 里的 onnxruntime.dll (System32 等).
            // LoadLibraryW 默认搜索顺序: exe dir > cwd > System32 > PATH.
            // 我们把 dll 拷到 release/ 之后这条路径命中; 若用户在 PATH 里配了
            // 系统 onnxruntime.dll, 直接用 System32 路径也可.
            if let Ok(exe) = std::env::current_exe() {
                if let Some(dir) = exe.parent() {
                    let cand = dir.join("onnxruntime.dll");
                    if cand.exists() {
                        return cand;
                    }
                }
            }
            PathBuf::from("onnxruntime.dll")
        } else {
            PathBuf::from("/usr/lib/libonnxruntime.so")
        }
    }
    pub fn new(model_path: PathBuf) -> anyhow::Result<Self> {
        log::info!(
            "init ONNX runtime from {:?}",
            Self::get_onnx_running_time_path()
        );
        ort::init_from(Self::get_onnx_running_time_path())?;
        log::info!("start build session");
        let session = ort::session::Session::builder()?.commit_from_file(model_path)?;
        log::info!("build session finished");
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

        // RgbImage::from_raw 需要 owned Vec, 这里 to_vec 一次 —
        // 与原调用方 clone 的拷贝量相同, PC 路径性能不变
        let input_data = preprocess_image(
            image_data.to_vec(),
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
        Ok(postprocess_output(
            &output_slice,
            scale,
            width,
            height,
            self.conf_threshold,
        ))
    }
}

pub fn preprocess_image(
    image_data: Vec<u8>,
    img_width: u32,
    img_height: u32,
    input_width: u32,
    input_height: u32,
) -> anyhow::Result<Vec<f32>> {
    let img_buffer = RgbImage::from_raw(img_width, img_height, image_data)
        .ok_or_else(|| anyhow::anyhow!("Failed to create image buffer"))?;
    let dynamic_img = DynamicImage::ImageRgb8(img_buffer);

    let scale =
        (input_width as f32 / img_width as f32).min(input_height as f32 / img_height as f32);
    let nw = (img_width as f32 * scale) as u32;
    let nh = (img_height as f32 * scale) as u32;

    let resized = dynamic_img
        .resize_exact(nw, nh, image::imageops::FilterType::Triangle)
        .to_rgb8();

    let mut canvas = RgbImage::from_pixel(input_width, input_height, Rgb([114, 114, 114]));
    image::imageops::overlay(&mut canvas, &resized, 0, 0);

    // NHWC 格式 (Height, Width, Channel) - 使用 0-255 范围
    let mut input = vec![0.0f32; (3 * input_width * input_height) as usize];
    let area = (input_width * input_height) as usize;
    for (i, pixel) in canvas.pixels().enumerate() {
        let x = (i as u32) % input_width;
        let y = (i as u32) / input_width;
        let idx = (y * input_width + x) as usize;
        input[idx] = f32::from(pixel.0[0]);
        input[idx + area] = f32::from(pixel.0[1]);
        input[idx + 2 * area] = f32::from(pixel.0[2]);
    }

    Ok(input)
}

/// 后处理 - 将模型输出转换为检测结果
#[must_use]
pub fn postprocess_output(
    output_slice: &[f32],
    scale: f32,
    img_width: u32,
    img_height: u32,
    conf_threshold: f32,
) -> Vec<FaceDetectionResult> {
    let num_anchors = 8400;
    let mut results = Vec::new();

    for i in 0..num_anchors.min(output_slice.len() / 5) {
        let score = output_slice[4 * num_anchors + i];
        if score > conf_threshold {
            let box_data = [
                output_slice[i],
                output_slice[num_anchors + i],
                output_slice[2 * num_anchors + i],
                output_slice[3 * num_anchors + i],
            ];

            let x_orig_px = box_data[0] / scale;
            let y_orig_px = box_data[1] / scale;
            let w_orig_px = box_data[2] / scale;
            let h_orig_px = box_data[3] / scale;

            results.push(FaceDetectionResult {
                has_face: true,
                x: x_orig_px / img_width as f32,
                y: y_orig_px / img_height as f32,
                width: w_orig_px / img_width as f32,
                height: h_orig_px / img_height as f32,
                confidence: score,
            });
        }
    }

    results.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    nms_filter(results, 0.5)
}
impl OrtFaceDetector {
    fn extract_output(outputs: &ort::session::SessionOutputs) -> anyhow::Result<Vec<f32>> {
        let output = &outputs[0];
        let arr = output.try_extract_array::<f32>()?;
        let view = arr.view();
        Ok(view.as_slice().unwrap_or(&[]).to_vec())
    }
}
