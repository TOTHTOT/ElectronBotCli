//! 视频模块 - 图像处理

use std::convert::TryFrom;

/// 旋转角度
#[derive(Debug, Clone, Copy, Default)]
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

/// RGB 转换为 BGR
#[inline]
pub fn rgb_to_bgr(rgb_data: &[u8], _width: u32, _height: u32) -> Vec<u8> {
    let mut bgr_data = Vec::with_capacity(rgb_data.len());
    for chunk in rgb_data.chunks_exact(3) {
        bgr_data.extend_from_slice(&[chunk[2], chunk[1], chunk[0]]);
    }
    bgr_data
}

/// 顺时针旋转 90 度
/// 原始 width x height -> 旋转后 height x width
pub fn rotate_90_cw(bgr_data: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut rotated = Vec::with_capacity((width * height * 3) as usize);

    for x in 0..width {
        for y in (0..height).rev() {
            let idx = ((y * width + x) * 3) as usize;
            if idx + 2 < bgr_data.len() {
                rotated.extend_from_slice(&[bgr_data[idx], bgr_data[idx + 1], bgr_data[idx + 2]]);
            }
        }
    }

    rotated
}

/// 顺时针旋转 180 度
pub fn rotate_180(bgr_data: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut rotated = Vec::with_capacity(bgr_data.len());

    for y in (0..height).rev() {
        for x in (0..width).rev() {
            let idx = ((y * width + x) * 3) as usize;
            if idx + 2 < bgr_data.len() {
                rotated.extend_from_slice(&[bgr_data[idx], bgr_data[idx + 1], bgr_data[idx + 2]]);
            }
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
            let idx = ((y * width + x) * 3) as usize;
            if idx + 2 < bgr_data.len() {
                rotated.extend_from_slice(&[bgr_data[idx], bgr_data[idx + 1], bgr_data[idx + 2]]);
            }
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

/// 处理视频帧（如添加人脸框）
pub fn process_frame(bgr_data: Vec<u8>, _width: u32, _height: u32) -> Vec<u8> {
    // TODO: 添加人脸检测功能
    bgr_data
}
