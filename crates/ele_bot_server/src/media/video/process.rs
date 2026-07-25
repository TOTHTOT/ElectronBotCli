//! 视频模块 - 图像处理

use crate::vision::face::{draw_hollow_rect_static, FaceDetectorTrait};
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
use crate::media::video::rga_adapter::RgaHelper;
use crate::media::video::types::{FrameData, FrameInfo};
use bytes::Bytes;
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
use librga::usage::Rotation;
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
use std::sync::OnceLock;

// RGA 辅助单例 (仅在 aarch64 Linux 上存在)
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
static RGA_HELPER: OnceLock<RgaHelper> = OnceLock::new();

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
    #[must_use] 
    pub fn from_degrees(degrees: u32) -> Self {
        match degrees % 360 {
            90 => Self::Rotate90,
            180 => Self::Rotate180,
            270 => Self::Rotate270,
            _ => Self::None,
        }
    }

    /// 是否需要交换宽高
    #[must_use] 
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
#[must_use] 
pub fn rotate_90_cw(bgr_data: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut rotated = Vec::with_capacity((width * height * 3) as usize);
    (0..width)
        .flat_map(|x| (0..height).rev().map(move |y| (x, y)))
        .for_each(|(x, y)| copy_pixel(bgr_data, x, y, width, &mut rotated));
    rotated
}

/// 顺时针旋转 180 度
#[must_use] 
pub fn rotate_180(bgr_data: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut rotated = Vec::with_capacity(bgr_data.len());
    (0..height)
        .rev()
        .flat_map(|y| (0..width).rev().map(move |x| (x, y)))
        .for_each(|(x, y)| copy_pixel(bgr_data, x, y, width, &mut rotated));
    rotated
}

/// 顺时针旋转 270 度（等同于逆时针 90 度）
/// 原始 width x height -> 旋转后 height x width
#[must_use] 
pub fn rotate_270_cw(bgr_data: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut rotated = Vec::with_capacity((width * height * 3) as usize);
    (0..width)
        .rev()
        .flat_map(|x| (0..height).map(move |y| (x, y)))
        .for_each(|(x, y)| copy_pixel(bgr_data, x, y, width, &mut rotated));
    rotated
}

/// 根据旋转角度处理图像
/// 优先使用 RGA 硬件加速，失败时回退到软件实现
#[must_use] 
pub fn rotate_by_angle(bgr_data: &[u8], width: u32, height: u32, angle: RotateAngle) -> Vec<u8> {
    // 尝试 RGA 硬件加速 (仅在 aarch64 Linux 上可用)
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        if angle != RotateAngle::None {
            let rotation = match angle {
                RotateAngle::Rotate90 => Some(Rotation::Rot90),
                RotateAngle::Rotate180 => Some(Rotation::Rot180),
                RotateAngle::Rotate270 => Some(Rotation::Rot270),
                RotateAngle::None => None,
            };

            if let Some(rot) = rotation {
                let helper = RGA_HELPER.get_or_init(RgaHelper::new);
                if let Some(result) = helper.rotate(bgr_data.to_vec(), width, height, rot) {
                    log::debug!("Using RGA hardware rotation for {:?}", angle);
                    return result;
                }
            }
        }
    }

    // 回退到软件实现
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
) -> anyhow::Result<FrameInfo> {
    // 尝试检测人脸
    let result = face_detector.detect(rgb_data.clone(), width, height)?;
    if result.has_face {
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
    Ok(FrameInfo {
        face_info: result,
        frame_data: FrameData::RawRgb(Bytes::from(rgb_data)),
        focused: false,
        emotion: boteyes::Mood::Default,
    })
}
