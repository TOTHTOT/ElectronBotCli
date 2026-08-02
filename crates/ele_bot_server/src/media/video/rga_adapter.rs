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

    /// YUYV -> RGB888 CSC + 旋转 + 缩放, 单次 RGA 硬件 pass.
    ///
    /// 640x480 全帧 CSC+旋转实测 ~1.3-2.2ms (含 buffer 包装), 替代软件
    /// LUT 融合版的 ~6.5ms; 检测输入的 CSC+旋转+缩到 320x320 一趟 ~1.2ms,
    /// 替代 "全帧转换 + 再 resize" 两趟. 失败返回 None, 调用方回退软件路径.
    ///
    /// 注意: 设备系统自带 librga 是 rga_api 1.3.2, 对 YUYV CSC 输出全绿
    /// (彩条隔离测试证实); 必须用 assets/lib 随包部署的官方 1.10.6,
    /// 二进制靠 $ORIGIN rpath 加载同目录的它.
    ///
    /// 缩放为拉伸 (不保比例), 与检测器现有的预处理行为一致.
    pub fn yuyv_to_rgb(
        &self,
        yuyv: &[u8],
        width: u32,
        height: u32,
        dst_w: u32,
        dst_h: u32,
        rotation: Option<Rotation>,
    ) -> Option<Vec<u8>> {
        if !self.available {
            return None;
        }

        debug_assert!(
            yuyv.len() >= (width * height * 2) as usize,
            "src slice too small for {width}x{height} YUYV"
        );
        // SAFETY: 与 resize() 的零拷贝相同 — yuyv 是活切片, process 同步
        // 执行完才返回, RGA 对 src 只读, *mut 仅为 FFI 签名要求.
        let src_buf = match unsafe {
            RgaBuffer::from_virtual_addr_unchecked(
                yuyv.as_ptr() as *mut std::ffi::c_void,
                width as i32,
                height as i32,
                PixelFormat::Yuyv422,
            )
        } {
            Ok(buf) => buf,
            Err(e) => {
                log::warn!("Failed to create RGA YUYV src buffer: {:?}", e);
                return None;
            }
        };

        let dst_data = vec![0u8; (dst_w * dst_h * 3) as usize];
        let (mut dst_buf, dst_owned) = match RgaBuffer::from_vec_mut(
            dst_data,
            dst_w as i32,
            dst_h as i32,
            PixelFormat::Rgb888,
        ) {
            Ok(buf) => buf,
            Err(e) => {
                log::warn!("Failed to create RGA dst buffer: {:?}", e);
                return None;
            }
        };

        let usage = rotation.map_or_else(librga::Usage::empty, |r| r.to_usage());
        let result = librga::process(
            &src_buf,
            &mut dst_buf,
            None,
            librga::rect::Rect::at_origin(width as i32, height as i32),
            librga::rect::Rect::at_origin(dst_w as i32, dst_h as i32),
            None,
            usage,
        );

        match result {
            Ok(_) => Some(dst_owned),
            Err(e) => {
                log::warn!("RGA yuyv_to_rgb failed: {:?}", e);
                None
            }
        }
    }

    /// 硬件缩放
    /// 将图像从 src_w x src_h 缩放到 dst_w x dst_h
    ///
    /// src 按 `&[u8]` 借用, 内部直接用原指针包 RGA buffer (零拷贝) —
    /// 每帧省掉一次全图 memcpy (~900KB @ 640x480).
    pub fn resize(
        &self,
        src_data: &[u8],
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
        debug_assert!(
            src_data.len() >= (src_w * src_h * 3) as usize,
            "src slice too small for {src_w}x{src_h} RGB888"
        );
        // SAFETY:
        // 1. src_data 是活切片, 指针对 src_data.len() 字节有效;
        //    debug_assert 保证 RGA 读取范围 (src_w*src_h*3) 不越界.
        // 2. librga resize 是同步调用 (imresize_t sync=1, 见 librga-rs
        //    ops/resize.rs), 函数返回时硬件已读完, 不存在 slice 释放后
        //    硬件还在读的 use-after-free.
        // 3. RGA 对 src buffer 只读; 共享切片 &[] 保证无 &mut 别名,
        //    *mut 转换仅是 FFI 签名要求.
        let src_buf = match unsafe {
            RgaBuffer::from_virtual_addr_unchecked(
                src_data.as_ptr() as *mut std::ffi::c_void,
                src_w as i32,
                src_h as i32,
                PixelFormat::Rgb888,
            )
        } {
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
