//! 视频模块 - 摄像头捕获

use bytes::Bytes;

use crate::media::video::process::{process_frame, rgb_to_bgr, rotate_by_angle, RotateAngle};
use crate::media::video::types::{CameraFormat as LocalCameraFormat, FrameCache, FrameData};
use nokhwa::pixel_format::RgbFormat;
use nokhwa::utils::{
    ApiBackend, CameraIndex, CameraInfo, FrameFormat, RequestedFormat, RequestedFormatType,
};
use nokhwa::{query, Camera};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[cfg(feature = "fps-counter")]
use std::time::Instant;

/// 帧率计算器
#[cfg(feature = "fps-counter")]
struct FrameRateCounter {
    last_time: Instant,
    frame_count: u32,
}

#[cfg(feature = "fps-counter")]
impl FrameRateCounter {
    fn new() -> Self {
        Self {
            last_time: Instant::now(),
            frame_count: 0,
        }
    }

    /// 计算帧率，返回 Some(fps) 如果已超过 1 秒
    fn tick(&mut self) -> Option<f64> {
        self.frame_count += 1;
        if self.last_time.elapsed().as_secs() >= 1 {
            let fps = self.frame_count as f64 / self.last_time.elapsed().as_secs_f64();
            self.frame_count = 0;
            self.last_time = Instant::now();
            Some(fps)
        } else {
            None
        }
    }
}

/// 视频捕获器 - 使用共享缓存
pub struct VideoCapture {
    /// 帧缓存
    frame_cache: FrameCache,
    /// 运行标志
    running: Arc<AtomicBool>,
    /// 摄像头名称
    device_name: Option<String>,
    /// 实际分辨率
    resolution: Arc<Mutex<(u32, u32)>>,
    /// 旋转角度
    rotate_angle: RotateAngle,
}

#[allow(dead_code)]
impl VideoCapture {
    /// 创建新的视频捕获器
    pub fn new(device_name: Option<String>) -> Self {
        log::info!("Creating VideoCapture with device: {device_name:?}");

        // 检测是否需要旋转
        let rotate_angle = if device_name
            .as_ref()
            .map(|name| name.contains("USB 2.0 PC Cam"))
            .unwrap_or(false)
        {
            RotateAngle::Rotate270
        } else {
            RotateAngle::None
        };

        if !matches!(rotate_angle, RotateAngle::None) {
            log::info!("Camera requires {:?} rotation", rotate_angle);
        }

        Self {
            frame_cache: Arc::new(Mutex::new(None)),
            running: Arc::new(AtomicBool::new(false)),
            device_name,
            resolution: Arc::new(Mutex::new((0, 0))),
            rotate_angle,
        }
    }

    /// 设置旋转角度
    pub fn set_rotate_angle(&mut self, angle: RotateAngle) {
        self.rotate_angle = angle;
        if !matches!(angle, RotateAngle::None) {
            log::info!("Rotation set to {:?}", angle);
        }
    }

    /// 获取旋转角度
    pub fn rotate_angle(&self) -> RotateAngle {
        self.rotate_angle
    }

    /// 获取帧缓存
    pub fn frame_cache(&self) -> FrameCache {
        self.frame_cache.clone()
    }

    /// 获取实际分辨率
    pub fn resolution(&self) -> (u32, u32) {
        *self.resolution.lock().unwrap()
    }

    /// 获取分辨率的 Arc 句柄（用于跨线程共享）
    pub fn resolution_arc(&self) -> Arc<Mutex<(u32, u32)>> {
        self.resolution.clone()
    }

    /// 开始捕获视频帧
    pub fn start_capture_frames_thread(&mut self) {
        if self.running.load(Ordering::Relaxed) {
            log::warn!("Video capture already running");
            return;
        }
        self.running.store(true, Ordering::Relaxed);

        let device_name = self.device_name.clone();
        let frame_cache = self.frame_cache.clone();
        let running = self.running.clone();
        let resolution = self.resolution.clone();
        let rotate_angle = self.rotate_angle;

        std::thread::spawn(move || {
            capture_frames(device_name, frame_cache, running, resolution, rotate_angle);
        });

        log::info!("Video capture started");
    }

    /// 停止捕获
    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
        log::info!("Video capture stop requested");
    }

    /// 列出可用摄像头
    pub fn list_cameras() -> anyhow::Result<Vec<CameraInfo>> {
        Ok(query(ApiBackend::Auto)?)
    }

    /// 获取摄像头支持的格式列表
    pub fn get_supported_formats(device_index: usize) -> Vec<LocalCameraFormat> {
        let mut formats = Vec::new();

        let index = CameraIndex::Index(device_index as u32);
        let query = RequestedFormat::new::<RgbFormat>(RequestedFormatType::None);

        if let Ok(mut camera) = Camera::new(index, query) {
            if let Ok(nokhwa_formats) = camera.compatible_camera_formats() {
                for fmt in nokhwa_formats {
                    formats.push(LocalCameraFormat {
                        width: fmt.width(),
                        height: fmt.height(),
                        fps: fmt.frame_rate(),
                        format_desc: fmt.format().to_string(),
                    });
                }
            }
        }
        formats
    }
}

/// 打开摄像头
fn open_camera_default(device_name: Option<&str>) -> anyhow::Result<Camera> {
    let cameras = VideoCapture::list_cameras()?;
    log::info!("Available cameras: {:?}", cameras);

    // 根据名称查找对应的索引
    let index = match device_name {
        Some(name) => {
            // 查找匹配的摄像头
            if let Some(camera_info) = cameras.iter().find(|c| c.human_name() == name) {
                log::info!("Found camera '{}' at index {:?}", name, camera_info.index());
                CameraIndex::Index(camera_info.index().as_index()?)
            } else {
                // 如果找不到，尝试使用索引 0
                log::warn!("Camera '{}' not found, falling back to index 0", name);
                CameraIndex::Index(0)
            }
        }
        None => CameraIndex::Index(0),
    };

    let query = RequestedFormat::new::<RgbFormat>(RequestedFormatType::None);
    Ok(Camera::new(index, query)?)
}

/// 捕获帧循环
fn capture_frames(
    device_name: Option<String>,
    frame_cache: FrameCache,
    running: Arc<AtomicBool>,
    resolution: Arc<Mutex<(u32, u32)>>,
    rotate_angle: RotateAngle,
) {
    let mut camera = match open_camera_default(device_name.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            log::error!("Could not open camera in capture loop, error: {e}");
            return;
        }
    };

    // 获取摄像头的信息
    let camera_fmt = camera.camera_format();
    log::info!("camera info: {:?}", camera_fmt);
    let width = camera_fmt.width();
    let height = camera_fmt.height();

    // 如果需要旋转 90 或 270 度，宽高交换
    let (out_width, out_height) = if rotate_angle.needs_swap() {
        (height, width)
    } else {
        (width, height)
    };
    *resolution.lock().unwrap() = (out_width, out_height);
    let format = camera_fmt.format();
    log::info!(
        "Camera resolution: {}x{} (rotate: {:?}, output: {}x{})",
        width,
        height,
        rotate_angle,
        out_width,
        out_height
    );

    // 帧率计算
    #[cfg(feature = "fps-counter")]
    let mut fps_counter = FrameRateCounter::new();

    while running.load(Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_millis(10));
        let frame = match camera.frame() {
            Ok(f) => f,
            Err(e) => {
                log::error!("Camera frame error: {:?}", e);
                std::thread::sleep(std::time::Duration::from_millis(100));
                continue;
            }
        };
        // 根据格式处理帧数据（使用输出宽高）
        let frame_data = process_frame_by_format(
            frame,
            out_width,
            out_height,
            format,
            rotate_angle,
            width,
            height,
        );

        // 计算帧率
        #[cfg(feature = "fps-counter")]
        if let Some(fps) = fps_counter.tick() {
            log::info!("Camera FPS: {:.1}", fps);
        }

        // 写入帧缓存
        let mut guard = frame_cache.lock().unwrap();
        *guard = Some(frame_data);
    }

    log::info!("Video capture loop stopped");
}

/// 根据帧格式处理数据
/// - out_width, out_height: 输出图像的宽高（已考虑旋转后的交换）
/// - rotate_angle: 旋转角度
/// - src_width, src_height: 原始图像的宽高（用于旋转计算）
fn process_frame_by_format(
    frame: nokhwa::Buffer,
    out_width: u32,
    out_height: u32,
    format: FrameFormat,
    rotate_angle: RotateAngle,
    src_width: u32,
    src_height: u32,
) -> FrameData {
    match format {
        FrameFormat::MJPEG => {
            // MJPEG 已经是压缩的 JPEG 数据，无法直接旋转，返回原始数据
            log::debug!(
                "Frame: {out_width}x{out_height}, MJPEG, {} bytes",
                frame.buffer().len()
            );
            FrameData::Jpeg(Bytes::copy_from_slice(frame.buffer()))
        }
        FrameFormat::YUYV | FrameFormat::NV12 => {
            // YUV 格式需要解码
            log::debug!(
                "Frame: {out_width}x{out_height}, len: {}, YUV, decoding...",
                frame.buffer().len()
            );
            let rgb_data = match frame.decode_image::<RgbFormat>() {
                Ok(img_buf) => img_buf.into_raw(),
                Err(e) => {
                    log::warn!("Failed to decode YUV frame: {:?}", e);
                    return FrameData::RawBgr(Bytes::new());
                }
            };
            // RGB -> BGR（使用原始尺寸）
            let mut bgr = rgb_to_bgr(&rgb_data, src_width, src_height);
            bgr = process_frame(bgr, src_width, src_height);

            // 应用旋转
            if !matches!(rotate_angle, RotateAngle::None) {
                bgr = rotate_by_angle(&bgr, src_width, src_height, rotate_angle);
            }

            FrameData::RawBgr(Bytes::from(bgr))
        }
        FrameFormat::RAWRGB | FrameFormat::RAWBGR => {
            // 原始 RGB/BGR 格式
            log::debug!("Frame: {}x{}, Raw RGB/BGR", out_width, out_height);
            let bgr = if format == FrameFormat::RAWBGR {
                Bytes::copy_from_slice(frame.buffer())
            } else {
                Bytes::from(rgb_to_bgr(frame.buffer(), src_width, src_height))
            };
            let mut processed = process_frame(bgr.to_vec(), src_width, src_height);

            // 应用旋转
            if !matches!(rotate_angle, RotateAngle::None) {
                processed = rotate_by_angle(&processed, src_width, src_height, rotate_angle);
            }

            FrameData::RawBgr(Bytes::from(processed))
        }
        FrameFormat::GRAY => {
            // 灰度格式
            log::debug!("Frame: {}x{}, GRAY", out_width, out_height);
            FrameData::RawBgr(Bytes::copy_from_slice(frame.buffer()))
        }
    }
}
