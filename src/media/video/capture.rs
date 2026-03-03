//! 视频模块 - 摄像头捕获

use crate::media::video::encode::bgr_to_jpeg;
use crate::media::video::process::{process_frame, rgb_to_bgr};
use crate::media::video::types::{
    CameraFormat as LocalCameraFormat, CameraInfo, FrameCache, FrameData,
};

use nokhwa::pixel_format::RgbFormat;
use nokhwa::utils::{CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType};
use nokhwa::Camera;

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

/// 视频捕获器
pub struct VideoCapture {
    frame_cache: FrameCache, // 帧缓存
    /// 运行标志
    running: Arc<AtomicBool>,
    /// 摄像头名称
    device_name: Option<String>,
    /// 实际分辨率
    resolution: Arc<Mutex<(u32, u32)>>,
}

#[allow(dead_code)]
impl VideoCapture {
    /// 创建新的视频捕获器
    pub fn new(device_name: Option<String>) -> Self {
        log::info!("Creating VideoCapture with device: {device_name:?}");

        Self {
            frame_cache: Arc::new(Mutex::new(None)),
            running: Arc::new(AtomicBool::new(false)),
            device_name,
            resolution: Arc::new(Mutex::new((0, 0))),
        }
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

        std::thread::spawn(move || {
            capture_frames(device_name, frame_cache, running, resolution);
        });

        log::info!("Video capture started");
    }

    /// 停止捕获
    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
        log::info!("Video capture stop requested");
    }

    /// 列出可用摄像头
    pub fn list_cameras() -> Vec<CameraInfo> {
        let mut cameras = Vec::new();

        for i in 0..10 {
            let index = CameraIndex::Index(i as u32);
            let query = RequestedFormat::new::<RgbFormat>(RequestedFormatType::None);

            if Camera::new(index, query).is_ok() {
                cameras.push(CameraInfo {
                    index: i.to_string(),
                    friendly_name: None,
                });
            }
        }

        cameras
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

    /// 获取当前帧的 JPEG 编码
    /// 如果数据已经是 JPEG（MJPEG 格式），直接返回
    /// 否则编码为 JPEG
    pub fn get_jpeg_frame(&self, quality: u8) -> Option<Vec<u8>> {
        let guard = self.frame_cache.lock().ok()?;
        let frame_data = guard.as_ref()?;

        // 如果已经是 JPEG，直接返回
        if let Some(jpeg) = frame_data.as_jpeg() {
            return Some(jpeg.clone());
        }

        // 否则需要编码
        let bgr_data = frame_data.as_raw_bgr()?;
        let (width, height) = self.resolution();
        if width == 0 || height == 0 {
            return None;
        }
        bgr_to_jpeg(bgr_data, width, height, quality)
    }
}

/// 打开摄像头
fn open_camera_default(device_name: Option<&str>) -> Option<Camera> {
    let index = match device_name {
        Some(name) => CameraIndex::String(name.to_string()),
        None => CameraIndex::Index(0),
    };

    let query = RequestedFormat::new::<RgbFormat>(RequestedFormatType::None);
    match Camera::new(index, query) {
        Ok(c) => {
            log::info!("Camera opened with auto format");
            Some(c)
        }
        Err(e) => {
            log::error!("Failed to open camera: {e:?}");
            None
        }
    }
}

/// 捕获帧循环
fn capture_frames(
    device_name: Option<String>,
    frame_cache: FrameCache,
    running: Arc<AtomicBool>,
    resolution: Arc<Mutex<(u32, u32)>>,
) {
    let mut camera = match open_camera_default(device_name.as_deref()) {
        Some(c) => c,
        None => {
            log::error!("Could not open camera in capture loop");
            return;
        }
    };

    // 获取摄像头的信息
    let camera_fmt = camera.camera_format();
    log::info!("camera info: {:?}", camera_fmt);
    let width = camera_fmt.width();
    let height = camera_fmt.height();
    *resolution.lock().unwrap() = (width, height);
    let format = camera_fmt.format();
    log::info!("Camera resolution: {width}x{height}, format: {format}");

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
        // 根据格式处理帧数据
        let frame_data = process_frame_by_format(frame, width, height, format);

        // 计算帧率
        #[cfg(feature = "fps-counter")]
        if let Some(fps) = fps_counter.tick() {
            log::info!("Camera FPS: {:.1}", fps);
        }

        let mut guard = frame_cache.lock().unwrap();
        *guard = Some(frame_data);
    }

    log::info!("Video capture loop stopped");
}

/// 根据帧格式处理数据
fn process_frame_by_format(
    frame: nokhwa::Buffer,
    width: u32,
    height: u32,
    format: FrameFormat,
) -> FrameData {
    match format {
        FrameFormat::MJPEG => {
            // MJPEG 已经是压缩的 JPEG 数据，浏览器可直接显示
            log::debug!(
                "Frame: {}x{}, MJPEG, {} bytes",
                width,
                height,
                frame.buffer().len()
            );
            FrameData::Jpeg(frame.buffer().to_vec())
        }
        FrameFormat::YUYV | FrameFormat::NV12 => {
            // YUV 格式需要解码
            log::debug!("Frame: {width}x{height}, YUV, decoding...");
            let rgb_data = match frame.decode_image::<RgbFormat>() {
                Ok(img_buf) => img_buf.into_raw(),
                Err(e) => {
                    log::warn!("Failed to decode YUV frame: {:?}", e);
                    return FrameData::RawBgr(Vec::new());
                }
            };
            // RGB -> BGR
            let bgr = rgb_to_bgr(&rgb_data, width, height);
            let processed = process_frame(bgr, width, height);
            FrameData::RawBgr(processed)
        }
        FrameFormat::RAWRGB | FrameFormat::RAWBGR => {
            // 原始 RGB/BGR 格式
            log::debug!("Frame: {}x{}, Raw RGB/BGR", width, height);
            let bgr = if format == FrameFormat::RAWBGR {
                frame.buffer().to_vec()
            } else {
                rgb_to_bgr(frame.buffer(), width, height)
            };
            let processed = process_frame(bgr, width, height);
            FrameData::RawBgr(processed)
        }
        FrameFormat::GRAY => {
            // 灰度格式
            log::debug!("Frame: {}x{}, GRAY", width, height);
            FrameData::RawBgr(frame.buffer().to_vec())
        }
    }
}
