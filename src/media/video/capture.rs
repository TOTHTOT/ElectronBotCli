//! 视频模块 - 摄像头捕获

use crate::media::video::process::{process_frame, rotate_by_angle, RotateAngle};
use crate::media::video::types::{CameraFormat as LocalCameraFormat, FrameCache, FrameInfo};
use crate::model_manager::ModelManager;
use crate::vision::face::create_face_detector;
use crate::vision::face::FaceDetectorTrait;
use anyhow::Context;
use bytes::Bytes;
use image::RgbImage;
use nokhwa::pixel_format::RgbFormat;
use nokhwa::utils::{
    ApiBackend, CameraFormat, CameraIndex, CameraInfo, FrameFormat, RequestedFormat,
    RequestedFormatType, Resolution,
};
use nokhwa::{query, Camera};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

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
    /// 摄像头索引
    camera_index: CameraIndex,
    /// 实际分辨率
    resolution: Arc<Mutex<(u32, u32)>>,
    /// 旋转角度
    rotate_angle: RotateAngle,
    /// 捕获线程句柄
    capture_thread: Option<JoinHandle<()>>,
}
/// Drop 时自动停止视频捕获线程并等待结束
impl Drop for VideoCapture {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.capture_thread.take() {
            log::info!("Dropping VideoCapture, waiting for thread...");
            let _ = handle.join();
            log::info!("VideoCapture dropped");
        }
    }
}

#[allow(dead_code)]
impl VideoCapture {
    /// 创建新的视频捕获器
    ///
    /// # Arguments
    /// * `camera_index` - 摄像头索引
    /// * `frame_cache` - 帧缓存通道
    /// * `rotate_angle` - 旋转角度
    pub fn new(
        camera_index: CameraIndex,
        frame_cache: FrameCache,
        rotate_angle: RotateAngle,
    ) -> Self {
        log::info!(
            "Creating VideoCapture with index: {camera_index:?}, rotation: {rotate_angle:?}"
        );

        Self {
            frame_cache,
            running: Arc::new(AtomicBool::new(false)),
            camera_index,
            resolution: Arc::new(Mutex::new((0, 0))),
            rotate_angle,
            capture_thread: None,
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
        self.resolution.lock().map(|guard| *guard).unwrap_or((0, 0))
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

        let camera_index = self.camera_index.clone();
        let frame_cache = self.frame_cache.clone();
        let running = self.running.clone();
        let resolution = self.resolution.clone();
        let rotate_angle = self.rotate_angle;

        let handle = thread::spawn(move || {
            capture_frames(camera_index, frame_cache, running, resolution, rotate_angle);
        });
        self.capture_thread = Some(handle);

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

/// 重置摄像头状态, 在rk3566的环境中, 第一次插入摄像头底层驱动没有配置帧率导致nokhwa会报错
/// `Could not get device property V4L2 FrameRate: Framerate not whole number: 1 / 0`
/// 现在如果是rk3566平台打开摄像头是先初始化一下
/// # Arguments
///
/// * `device_index`: 摄像头索引
///
/// returns: Result<(), Error>
///
/// # Examples
///
/// ```
/// ensure_camera_ready(0)?;
/// ```
#[allow(dead_code)]
fn ensure_camera_ready(device_index: &CameraIndex) -> anyhow::Result<()> {
    let CameraIndex::Index(idx) = device_index else {
        anyhow::bail!("Device index does not exist");
    };
    let device_path = format!("/dev/video{}", idx);

    if !Path::new(&device_path).exists() {
        return Err(anyhow::anyhow!("can not find camera: {}", device_path));
    }

    log::info!("正在唤醒摄像头驱动 {} ...", device_path);

    let status = Command::new("v4l2-ctl")
        .args([
            "-d",
            &device_path,
            "--set-fmt-video=width=640,height=480,pixelformat=YUYV",
            "--set-parm=30",
        ])
        .status()?; // 获取执行状态

    if !status.success() {
        log::warn!("v4l2-ctl fail v4l-utils");
    }

    thread::sleep(Duration::from_millis(100));

    Ok(())
}

/// 打开摄像头
fn open_camera_default(index: CameraIndex) -> anyhow::Result<Camera> {
    log::info!("Opening camera with index: {:?}", index);
    // 构建指定要求的摄像头参数 480p, yuyv格式, 30帧, 如果拿不到这个要求的配置直接报错, 修复了摄像头不配置帧率会报错的问题
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    let _ = ensure_camera_ready(&index);
    let format_type = RequestedFormatType::Exact(CameraFormat::new(
        Resolution {
            width_x: 640,
            height_y: 480,
        },
        FrameFormat::YUYV,
        30,
    ));
    log::info!("cameras: {:#?}", VideoCapture::list_cameras());
    let query = RequestedFormat::new::<RgbFormat>(format_type);
    Ok(Camera::new(index, query)?)
}

/// 捕获帧循环
fn capture_frames(
    camera_index: CameraIndex,
    frame_cache: FrameCache,
    running: Arc<AtomicBool>,
    resolution: Arc<Mutex<(u32, u32)>>,
    rotate_angle: RotateAngle,
) {
    let mut camera = match open_camera_default(camera_index) {
        Ok(mut c) => {
            if let Err(e) = c.open_stream().context("Failed to open camera stream") {
                log::error!("Could not open camera stream: {e}");
                return;
            }
            c
        }
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

    // 加载人脸检测器
    let Ok(mut face_detector) = get_face_detector() else {
        log::error!("Could not get face_detector");
        return;
    };

    // 如果需要旋转 90 或 270 度，宽高交换
    let (out_width, out_height) = if rotate_angle.needs_swap() {
        (height, width)
    } else {
        (width, height)
    };
    if let Ok(mut guard) = resolution.lock() {
        *guard = (out_width, out_height);
    }
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
        let frame = match camera.frame() {
            Ok(f) => f,
            Err(e) => {
                log::error!("Camera frame error: {:?}", e);
                thread::sleep(Duration::from_millis(100));
                continue;
            }
        };
        let start_time = Instant::now();
        // 拿到原始的图像数据然后根据格式处理帧数据
        let Ok(frame_data) = process_frame_by_format(
            frame,
            out_width,
            out_height,
            format,
            rotate_angle,
            width,
            height,
            &mut face_detector,
        ) else {
            continue;
        };
        log::debug!("process used time: {:?}", start_time.elapsed());

        // 计算帧率
        #[cfg(feature = "fps-counter")]
        if let Some(fps) = fps_counter.tick() {
            log::info!("Camera FPS: {:.1}", fps);
        }
        // 通过通道发送帧
        // 使用 broadcast
        let _ = frame_cache.send(frame_data);
    }

    log::info!("Video capture loop stopped");
}

/// 获取人脸检测器
fn get_face_detector() -> anyhow::Result<Box<dyn FaceDetectorTrait>> {
    log::info!("Loading face detector...");
    let mm = ModelManager::global();
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    let face_detect_path = mm.get("retinaface_rknn");
    #[cfg(not(all(target_os = "linux", target_arch = "aarch64")))]
    let face_detect_path = mm.get("yolo_face");
    let path = face_detect_path.ok_or_else(|| anyhow::anyhow!("yolo_face not found"))?;

    create_face_detector(path)
}

/// 处理帧并应用旋转
/// 新流程: 先旋转 -> 后检测 (坐标无需转换)
fn process_and_rotate(
    rgb: Vec<u8>,
    width: u32,
    height: u32,
    face_detector: &mut Box<dyn FaceDetectorTrait>,
    rotate_angle: RotateAngle,
) -> anyhow::Result<FrameInfo> {
    let start_time = Instant::now();
    let rotated = if rotate_angle == RotateAngle::None {
        rgb
    } else {
        rotate_by_angle(&rgb, width, height, rotate_angle)
    };
    let (new_width, new_height) = if rotate_angle.needs_swap() {
        (height, width)
    } else {
        (width, height)
    };
    log::debug!("rotate used time: {:?}", start_time.elapsed());

    let detect_start = Instant::now();
    let processed = process_frame(rotated, new_width, new_height, face_detector)?;
    log::debug!("process_frame used time: {:?}", detect_start.elapsed());
    Ok(processed)
}

/// 根据帧格式处理数据
///
/// # Arguments
///
/// * `frame`:
/// * `out_width`: 输出图像的宽
/// * `out_height`: 输出图像的高
/// * `format`: 图像格式
/// * `rotate_angle`: 旋转角度
/// * `src_width`: 摄像头的宽
/// * `src_height`: 摄像头的高
/// * `face_detector`:
///
/// returns: FrameData
///
/// # Examples
///
/// ```
///
/// ```
#[allow(clippy::too_many_arguments)]
fn process_frame_by_format(
    frame: nokhwa::Buffer,
    out_width: u32,
    out_height: u32,
    format: FrameFormat,
    rotate_angle: RotateAngle,
    src_width: u32,
    src_height: u32,
    face_detector: &mut Box<dyn FaceDetectorTrait>,
) -> anyhow::Result<FrameInfo> {
    match format {
        FrameFormat::YUYV | FrameFormat::NV12 => {
            log::debug!(
                "Frame: {out_width}x{out_height}, len: {}, YUV, decoding...",
                frame.buffer().len()
            );
            let Ok(rgb_data) = frame.decode_image::<RgbFormat>() else {
                anyhow::bail!("failed to decode image");
            };
            process_and_rotate(
                rgb_data.into_raw(),
                src_width,
                src_height,
                face_detector,
                rotate_angle,
            )
        }
        _ => {
            anyhow::bail!("Unsupported frame format {:?}", format);
        }
    }
}

/// 解码 JPEG 为 RGB 数据
#[allow(dead_code)]
fn decode_jpeg_to_rgb(jpeg_data: &[u8]) -> Option<Vec<u8>> {
    use std::io::Cursor;

    let cursor = Cursor::new(jpeg_data);
    let reader = image::ImageReader::with_format(cursor, image::ImageFormat::Jpeg);
    let image = reader.decode().ok()?;

    // 转换为 RGB8
    let rgb = image.to_rgb8();
    Some(rgb.into_raw())
}

/// 将 RGB 数据编码为 JPEG
#[allow(dead_code)]
fn encode_rgb_to_jpeg(rgb_data: &[u8], width: u32, height: u32) -> Option<Bytes> {
    use image::codecs::jpeg::JpegEncoder;
    use std::io::Cursor;

    let img = RgbImage::from_raw(width, height, rgb_data.to_vec())?;

    let mut output = Vec::new();
    let cursor = Cursor::new(&mut output);
    let mut encoder = JpegEncoder::new_with_quality(cursor, 85);

    if encoder.encode_image(&img).is_ok() {
        Some(Bytes::from(output))
    } else {
        None
    }
}
