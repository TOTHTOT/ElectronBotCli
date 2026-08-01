//! 视频模块 - 图像处理

use crate::vision::face::{draw_hollow_rect_static, FaceDetectionResult, FaceDetectorTrait};
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

// ---- YUYV -> RGB 查表 (BT.601 full range) ----
// A55 顺序小核上逐像素 i32 乘法不划算, 三个增量全部预计算成 LUT.
// const 上下文不允许函数指针/闭包调用, 用宏把表达式直接内联展开.
macro_rules! build_tab {
    ($v:ident => $e:expr) => {{
        let mut t = [0i32; 256];
        let mut i = 0;
        while i < 256 {
            let $v = i as i32 - 128;
            t[i] = $e;
            i += 1;
        }
        t
    }};
}
static R_TAB: [i32; 256] = build_tab!(v => (359 * v) >> 8);
static GU_TAB: [i32; 256] = build_tab!(v => 88 * v);
static GV_TAB: [i32; 256] = build_tab!(v => 183 * v);
static B_TAB: [i32; 256] = build_tab!(v => (454 * v) >> 8);

/// 解码一个 YUYV 4 字节组 (Y0 U Y1 V) 对应的 RGB 增量
#[inline(always)]
fn yuv_deltas(px: &[u8]) -> (i32, i32, i32) {
    let u = px[1] as usize;
    let v = px[3] as usize;
    (
        R_TAB[v],
        (GU_TAB[u] + GV_TAB[v]) >> 8,
        B_TAB[u],
    )
}

#[inline(always)]
fn clamp8(x: i32) -> u8 {
    x.clamp(0, 255) as u8
}

/// 手写 YUYV -> RGB888 解码, 替代 nokhwa `decode_image` (~10ms -> ~2ms).
///
/// BT.601 full range 查表整数运算, 按 6 字节块写入 (避免逐字节 push).
/// 颜色公式与 USB 摄像头 full-range YUYV 输出匹配.
#[must_use]
pub fn fast_yuyv_to_rgb(yuyv: &[u8], width: u32, height: u32) -> Vec<u8> {
    let npix = (width * height) as usize;
    let mut rgb = vec![0u8; npix * 3];
    for (px, out) in yuyv.chunks_exact(4).zip(rgb.chunks_exact_mut(6)) {
        let (r_d, g_d, b_d) = yuv_deltas(px);
        let y0 = i32::from(px[0]);
        let y1 = i32::from(px[2]);
        out[0] = clamp8(y0 + r_d);
        out[1] = clamp8(y0 - g_d);
        out[2] = clamp8(y0 + b_d);
        out[3] = clamp8(y1 + r_d);
        out[4] = clamp8(y1 - g_d);
        out[5] = clamp8(y1 + b_d);
    }
    rgb
}

/// YUYV 解码 + 旋转 270° 融合单 pass: 解码时直接写到旋转后位置.
///
/// 替代 "fast_yuyv_to_rgb (~2ms) + RGA rotate (~4.6ms)" 两步, 合计 ~3ms.
/// 输出尺寸 (height x width). 输入按行顺序读 (cache 友好),
/// 输出为列序写 — 900KB 输出驻留 L2, 代价可接受.
///
/// Rotate270 (逆时针 90°) 映射: 输入 (x, y) -> 输出 (x'=y, y'=w-1-x),
/// out_w = height, 故 out 偏移 = ((w-1-x)*height + y) * 3.
#[must_use]
pub fn fast_yuyv_to_rgb_rot270(yuyv: &[u8], width: u32, height: u32) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let mut rgb = vec![0u8; w * h * 3];
    for (i, px) in yuyv.chunks_exact(4).take(w * h / 2).enumerate() {
        let y = i / (w / 2);
        let x0 = (i % (w / 2)) * 2;
        let (r_d, g_d, b_d) = yuv_deltas(px);
        for (k, yv) in [i32::from(px[0]), i32::from(px[2])].into_iter().enumerate() {
            let x = x0 + k;
            let o = ((w - 1 - x) * h + y) * 3;
            rgb[o] = clamp8(yv + r_d);
            rgb[o + 1] = clamp8(yv - g_d);
            rgb[o + 2] = clamp8(yv + b_d);
        }
    }
    rgb
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
///
/// `face_detector` 为 `None` 时跳过检测: 若给了 `cached_face` 则复用上次
/// 结果 (隔帧检测: RKNN 推理 ~19ms 占满 30fps 帧预算, 隔一帧检测把均值
/// 压回 ~19ms, 框刷新 15Hz 肉眼无感), 否则为 default (`has_face=false`,
/// 这是 detector 创建超时/hang 的 fallback, 让帧照常出).
///
/// `&mut dyn` 借用而非 owned Box: 早期版本用 owned `Option<Box<dyn Trait>>`
/// + 调用方 `take()`, 导致 detector 在第一帧就被 drop, 之后检测静默失效.
///
/// 借用随函数返回结束, capture loop 里每帧可重复借用.
pub fn process_frame(
    mut rgb_data: Vec<u8>,
    width: u32,
    height: u32,
    face_detector: Option<&mut (dyn FaceDetectorTrait + 'static)>,
    cached_face: Option<&FaceDetectionResult>,
) -> anyhow::Result<FrameInfo> {
    // 尝试检测人脸 (借用原帧, 不再 clone)
    let result = if let Some(detector) = face_detector {
        detector.detect(&rgb_data, width, height)?
    } else {
        cached_face.cloned().unwrap_or_default()
    };
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
