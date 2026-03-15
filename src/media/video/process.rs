//! 视频模块 - 图像处理

use crate::vision::face::{draw_hollow_rect_static, FaceDetectorTrait};
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;

/// 旋转角度
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub enum RotateAngle {
    #[default]
    None,
    Rotate90,
    Rotate180,
    Rotate270,
}

impl RotateAngle {
    /// 根据角度值创建（0, 90, 180, 270）
    pub fn from_degrees(degrees: u32) -> Self {
        match degrees % 360 {
            90 => Self::Rotate90,
            180 => Self::Rotate180,
            270 => Self::Rotate270,
            _ => Self::None,
        }
    }

    /// 是否需要交换宽高
    pub fn needs_swap(&self) -> bool {
        matches!(self, Self::Rotate90 | Self::Rotate270)
    }
}

impl TryFrom<u32> for RotateAngle {
    type Error = String;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Ok(Self::from_degrees(value))
    }
}

/// 将像素从源缓冲区复制到目标向量
#[inline]
fn copy_pixel(src: &[u8], x: u32, y: u32, width: u32, dst: &mut Vec<u8>) {
    let idx = ((y * width + x) * 3) as usize;
    if idx + 2 < src.len() {
        dst.extend_from_slice(&src[idx..idx + 3]);
    }
}

/// 顺时针旋转 90 度
/// 原始 width x height -> 旋转后 height x width
pub fn rotate_90_cw(bgr_data: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut rotated = Vec::with_capacity((width * height * 3) as usize);
    for x in 0..width {
        for y in (0..height).rev() {
            copy_pixel(bgr_data, x, y, width, &mut rotated);
        }
    }
    rotated
}

/// 顺时针旋转 180 度
pub fn rotate_180(bgr_data: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut rotated = Vec::with_capacity(bgr_data.len());
    for y in (0..height).rev() {
        for x in (0..width).rev() {
            copy_pixel(bgr_data, x, y, width, &mut rotated);
        }
    }
    rotated
}

/// 顺时针旋转 270 度（等同于逆时针 90 度）
/// 原始 width x height -> 旋转后 height x width
pub fn rotate_270_cw(bgr_data: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut rotated = Vec::with_capacity((width * height * 3) as usize);
    for x in (0..width).rev() {
        for y in 0..height {
            copy_pixel(bgr_data, x, y, width, &mut rotated);
        }
    }
    rotated
}

/// 根据旋转角度处理图像
pub fn rotate_by_angle(bgr_data: &[u8], width: u32, height: u32, angle: RotateAngle) -> Vec<u8> {
    match angle {
        RotateAngle::None => bgr_data.to_vec(),
        RotateAngle::Rotate90 => rotate_90_cw(bgr_data, width, height),
        RotateAngle::Rotate180 => rotate_180(bgr_data, width, height),
        RotateAngle::Rotate270 => rotate_270_cw(bgr_data, width, height),
    }
}

/// 绘制人脸框到图像上
/// 使用 face 模块的统一画框函数
fn draw_face_box(bgr_data: &mut [u8], width: u32, height: u32, x: f32, y: f32, w: f32, h: f32) {
    let cx = (x * width as f32) as i32;
    let cy = (y * height as f32) as i32;
    let box_w = (w * width as f32) as i32;
    let box_h = (h * height as f32) as i32;

    let x1 = cx - box_w / 2;
    let y1 = cy - box_h / 2;

    const COLOR: [u8; 3] = [0, 255, 0];

    draw_hollow_rect_static(
        bgr_data,
        width,
        height,
        x1,
        y1,
        box_w as u32,
        box_h as u32,
        COLOR,
    );
}

/// 处理视频帧, 添加人脸检测和框
pub fn process_frame(
    mut rgb_data: Vec<u8>,
    width: u32,
    height: u32,
    face_detector: &mut Box<dyn FaceDetectorTrait>,
) -> Vec<u8> {
    // 尝试检测人脸
    match face_detector.detect(&rgb_data, width, height) {
        Ok(result) => {
            if result.has_face {
                // log::info!(
                //     "Face detected at ({:.2}, {:.2}) size {:.2}x{:.2}",
                //     result.x,
                //     result.y,
                //     result.width,
                //     result.height
                // );
                // 绘制人脸框
                draw_face_box(
                    &mut rgb_data,
                    width,
                    height,
                    result.x,
                    result.y,
                    result.width,
                    result.height,
                );
            }
        }
        Err(e) => {
            log::warn!("Face detection error: {}", e);
        }
    }
    rgb_data
}
