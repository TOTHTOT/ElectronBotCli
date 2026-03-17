//! RGA 硬件加速适配器
//! 提供基于 librga 的图像旋转和缩放功能

use librga::{ops::resize::ResizeOptions, usage::Rotation, PixelFormat, RgaBuffer};
use std::time::Instant;

/// RGA 硬件加速辅助类
pub struct RgaHelper {
    available: bool,
}

impl RgaHelper {
    pub fn new() -> Self {
        // 检查 RGA 设备是否存在
        let available = std::path::Path::new("/dev/rga").exists();
        if !available {
            log::warn!("RGA device not found, will use software fallback");
        }
        Self { available }
    }

    /// 硬件旋转 (90/180/270)
    /// 返回旋转后的数据，如果失败返回 None
    pub fn rotate(
        &self,
        src_data: Vec<u8>,
        width: u32,
        height: u32,
        rotation: Rotation,
    ) -> Option<Vec<u8>> {
        if !self.available {
            return None;
        }

        // 计算目标尺寸
        let (dst_width, dst_height) = match rotation {
            Rotation::Rot90 | Rotation::Rot270 => (height, width),
            Rotation::Rot180 => (width, height),
        };

        let dst_capacity = (dst_width * dst_height * 3) as usize;

        let t0 = Instant::now();
        let (src_buf, _src_owned) =
            match RgaBuffer::from_vec(src_data, width as i32, height as i32, PixelFormat::Rgb888) {
                Ok(buf) => buf,
                Err(e) => {
                    log::warn!("Failed to create RGA src buffer: {:?}", e);
                    return None;
                }
            };
        let t1 = Instant::now();

        let t2 = Instant::now();
        let dst_data = vec![0u8; dst_capacity];
        let (mut dst_buf, dst_owned) = match RgaBuffer::from_vec_mut(
            dst_data,
            dst_width as i32,
            dst_height as i32,
            PixelFormat::Rgb888,
        ) {
            Ok(buf) => buf,
            Err(e) => {
                log::warn!("Failed to create RGA dst buffer: {:?}", e);
                return None;
            }
        };
        let t3 = Instant::now();

        let t4 = Instant::now();
        let result = librga::rotate(&src_buf, &mut dst_buf, rotation, true);
        let t5 = Instant::now();

        log::debug!("rga rotate step1 create src buffer: {:?}", t1 - t0);
        log::debug!("rga rotate step2 create dst buffer: {:?}", t3 - t2);
        log::debug!("rga rotate step3 exec rotate: {:?}", t5 - t4);
        log::debug!(
            "rga rotate total: {}x{} -> {}x{}: {:?}",
            width,
            height,
            dst_width,
            dst_height,
            t5 - t0
        );

        match result {
            Ok(_) => Some(dst_owned),
            Err(e) => {
                log::warn!("RGA rotate failed: {:?}", e);
                None
            }
        }
    }

    /// 硬件缩放
    /// 将图像从 src_w x src_h 缩放到 dst_w x dst_h
    pub fn resize(
        &self,
        src_data: Vec<u8>,
        src_w: u32,
        src_h: u32,
        dst_w: u32,
        dst_h: u32,
    ) -> Option<Vec<u8>> {
        if !self.available {
            return None;
        }

        let dst_capacity = (dst_w * dst_h * 3) as usize;

        let t0 = Instant::now();
        let (src_buf, _src_owned) =
            match RgaBuffer::from_vec(src_data, src_w as i32, src_h as i32, PixelFormat::Rgb888) {
                Ok(buf) => buf,
                Err(e) => {
                    log::warn!("Failed to create RGA src buffer for resize: {:?}", e);
                    return None;
                }
            };
        let t1 = Instant::now();

        let t2 = Instant::now();
        let dst_data = vec![0u8; dst_capacity];
        let (mut dst_buf, dst_owned) = match RgaBuffer::from_vec_mut(
            dst_data,
            dst_w as i32,
            dst_h as i32,
            PixelFormat::Rgb888,
        ) {
            Ok(buf) => buf,
            Err(e) => {
                log::warn!("Failed to create RGA dst buffer for resize: {:?}", e);
                return None;
            }
        };
        let t3 = Instant::now();

        let t4 = Instant::now();
        let scale_x = dst_w as f64 / src_w as f64;
        let scale_y = dst_h as f64 / src_h as f64;
        let options = ResizeOptions::with_scale(scale_x, scale_y);
        let t5 = Instant::now();

        let t6 = Instant::now();
        let result = librga::resize(&src_buf, &mut dst_buf, options);
        let t7 = Instant::now();

        log::debug!("rga resize step1 create src buffer: {:?}", t1 - t0);
        log::debug!("rga resize step2 create dst buffer: {:?}", t3 - t2);
        log::debug!("rga resize step3 calc scale: {:?}", t5 - t4);
        log::debug!("rga resize step4 exec resize: {:?}", t7 - t6);
        log::debug!(
            "rga resize total: {}x{} -> {}x{}: {:?}",
            src_w,
            src_h,
            dst_w,
            dst_h,
            t7 - t0
        );

        match result {
            Ok(_) => Some(dst_owned),
            Err(e) => {
                log::warn!("RGA resize failed: {:?}", e);
                None
            }
        }
    }
}

impl Default for RgaHelper {
    fn default() -> Self {
        Self::new()
    }
}
