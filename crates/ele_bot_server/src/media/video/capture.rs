//! 视频模块 - 摄像头捕获

use crate::media::video::process::{process_frame, rotate_by_angle, RotateAngle};
use crate::media::video::types::{CameraFormat as LocalCameraFormat, FrameCache, FrameInfo};
use crate::model_manager::ModelManager;
use crate::vision::face::FaceDetectorTrait;
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

/// 把 `AppConfig.camera_index` 字符串解析成 `nokhwa` 可识别的 [`CameraIndex`].
///
/// 整数走 `CameraIndex::Index` (USB 常见), 非整数走 `CameraIndex::String`
/// (IPC / 路径). 失败时回退到 `CameraIndex::Index(0)`, 不报错 — 与
/// `SharedState::new` 原有语义对齐, 防止 config 损坏导致服务起不来.
#[must_use]
pub(crate) fn parse_camera_index(s: &str) -> CameraIndex {
    if let Ok(idx) = s.parse::<u32>() {
        CameraIndex::Index(idx)
    } else if s.is_empty() {
        CameraIndex::Index(0)
    } else {
        CameraIndex::String(s.to_string())
    }
}

/// 枚举系统所有摄像头, 转成 wire 上的 [`ele_bot_proto::CameraInfoDto`].
///
/// 当前没有任何 USB / IPC 摄像头时返回 `vec![]` (非 `Err`), 让调用方
/// (ws) 直接空回复 `ServerEvent::Cameras { cameras: vec![] }`, 客户端
/// 把这个状态当"无设备可选"展示, 不显示错误弹窗.
///
/// # Examples
///
/// ```rust,ignore
/// let cameras = list_cameras_dto();
/// for d in &cameras { log::info!("{}: {}", d.id, d.display); }
/// ```
#[must_use]
pub fn list_cameras_dto() -> Vec<ele_bot_proto::CameraInfoDto> {
    match VideoCapture::list_cameras() {
        Ok(list) => list.into_iter().map(|ci| camera_info_to_dto(&ci)).collect(),
        Err(e) => {
            log::warn!("list_cameras failed: {e:?}");
            Vec::new()
        }
    }
}

/// 单个 nokhwa `CameraInfo` -> `CameraInfoDto`.
///
/// 字段映射规则, 按 platform 上 nokhwa 0.10 实际行为校准:
///
/// - `nokhwa::CameraInfo::misc` 是设备硬件路径
///   (Windows MSMF 上是完整的 `\\?\usb#vid_...&pid_...#...\{GUID}\global`,
///   Linux V4L2 上是 `/dev/videoN` 类), 跟"驱动/后端名"不是一回事,
///   **不能直接展示给用户**.
/// - `description` 才是真正的人类可读"驱动/后端"标签
///   (Windows 上固定 `MediaFoundation Camera`,
///   macOS 上 `AVFoundation Camera`, Linux V4L2 上偶尔空).
/// - `human_name` 是设备厂商起的型号名 (`USB 2.0 PC Cam` 等),
///   主要识别位.
///
/// `display` 拼成 `<description-or-placeholder> <human_name> (id=<n>)`.
/// 缺 `human_name` 时退到 `Camera N`; 缺 `description` 时 driver 段也退到
/// `Camera N` 且跟 `name` 段合并避免重复.
fn camera_info_to_dto(ci: &CameraInfo) -> ele_bot_proto::CameraInfoDto {
    let id = ci.index().as_string();
    let human = ci.human_name();
    let desc = ci.description().to_string();

    let has_human = !human.trim().is_empty();
    let has_desc = !desc.trim().is_empty();
    // 两个字段给任何一个人类字符串就 OK. 都空时统一用 `Camera {id}` 占位,
    // 避免 DTO display 出现空字符串.
    let name = if has_human {
        human.clone()
    } else {
        format!("Camera {id}")
    };
    let driver = if has_desc {
        desc.clone()
    } else {
        format!("Camera {id}")
    };

    let display = if driver == name {
        // driver 段退化到与 name 段同名 (两者都退到 "Camera N" 时),
        // 避免输出 `Camera 0 Camera 0 (id=0)` 这种重复. 退化成单段.
        format!("Camera {id} (id={id})")
    } else {
        format!("{driver} {name} (id={id})")
    };
    ele_bot_proto::CameraInfoDto {
        id,
        // `name` 字段: 兜底匹配用, 沿用优先用 human_name, 没有就拿 description.
        name: if has_human { human } else { desc },
        display,
    }
}

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
    bus: FrameCache,
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
    #[must_use]
    pub fn new(camera_index: CameraIndex, bus: FrameCache, rotate_angle: RotateAngle) -> Self {
        log::info!(
            "Creating VideoCapture with index: {camera_index:?}, rotation: {rotate_angle:?}"
        );

        Self {
            bus,
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
            log::info!("Rotation set to {angle:?}");
        }
    }

    /// 获取旋转角度
    #[must_use]
    pub fn rotate_angle(&self) -> RotateAngle {
        self.rotate_angle
    }

    /// 获取帧缓存
    #[must_use]
    pub fn frame_cache(&self) -> FrameCache {
        self.bus.clone()
    }

    /// 获取实际分辨率
    #[must_use]
    pub fn resolution(&self) -> (u32, u32) {
        self.resolution.lock().map(|guard| *guard).unwrap_or((0, 0))
    }

    /// 获取分辨率的 Arc 句柄（用于跨线程共享）
    #[must_use]
    pub fn resolution_arc(&self) -> Arc<Mutex<(u32, u32)>> {
        self.resolution.clone()
    }

    /// 尝试开始捕获视频帧. **同步打开相机**, 失败时函数返回 Err
    /// 让调用方感知(对应 `SharedState::rebuild_video` 的 fallback 路径)
    /// — 旧版本把 `open_camera_default` 放后台 capture thread 里, 失败
    /// 只打 log, 表现成"switch OK 但黑屏", 用户没法切回去.
    ///
    /// ## 与 capture thread 两次打开的原因
    ///
    /// nokhwa 0.10 在 windows 上没编译 `unsafe impl Send for Camera`(需要
    /// `camera-sync-impl` feature, 我们项目用 `default-features = false`),
    /// 所以 `Camera` 不是 `Send`. 不能直接 spawn 把所有权 transfer 给 thread.
    /// 这里:
    /// 1. **探测一次** `open_camera_default` 拿到一个临时 `Camera`, 确认
    ///    设备能打开 & 格式正确; 这一步失败立刻 `?` 上抛, `rebuild_video`
    ///    走 fallback
    /// 2. 探测 `Camera` drop, 然后 spawn 新 capture thread, 在线程内部
    ///    第二次 `open_camera_default` 真正打开. 同设备两次打开互不冲突,
    ///    nokhwa 在 MSMF 后端用 reference counting.
    pub fn try_start_capture_frames_thread(&mut self) -> anyhow::Result<()> {
        if self.running.load(Ordering::Relaxed) {
            log::warn!("Video capture already running");
            return Ok(());
        }

        // 1. 探测打开. 失败 → Err 透传给 rebuild_video.
        self.detect_camera()?;

        self.running.store(true, Ordering::Relaxed);
        let camera_index = self.camera_index.clone();
        let frame_cache = self.bus.clone();
        let running = self.running.clone();
        let resolution = self.resolution.clone();
        let rotate_angle = self.rotate_angle;

        let handle = thread::spawn(move || {
            if let Err(e) =
                capture_frames(camera_index, frame_cache, running, resolution, rotate_angle)
            {
                log::error!("Capture thread panicked: {e:?}");
            }
        });
        self.capture_thread = Some(handle);
        log::info!("Video capture started");
        Ok(())
    }

    fn detect_camera(&mut self) -> anyhow::Result<()> {
        log::info!("Detecting camera, with index: {:?}", self.camera_index);
        let probe = open_camera_default(self.camera_index.clone())?;
        // 顺手读一次 camera_format, 把分辨率写进 self.resolution; 后续
        // capture thread 也会再写一次, 但先填好让外部早一点读到.
        let fmt = probe.camera_format();
        if let Ok(mut g) = self.resolution.lock() {
            let w = fmt.width();
            let h = fmt.height();
            let (out_w, out_h) = if self.rotate_angle.needs_swap() {
                (h, w)
            } else {
                (w, h)
            };
            *g = (out_w, out_h);
        }
        // probe 在 drop 时关闭 nokhwa MSMF session
        drop(probe);
        Ok(())
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
    #[must_use]
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
/// ```ignore
/// ensure_camera_ready(0)?;
/// ```
#[allow(dead_code)]
pub(crate) fn ensure_camera_ready(device_index: &CameraIndex) -> anyhow::Result<()> {
    let CameraIndex::Index(idx) = device_index else {
        anyhow::bail!("Device index does not exist");
    };
    let device_path = format!("/dev/video{idx}");

    if !Path::new(&device_path).exists() {
        return Err(anyhow::anyhow!("can not find camera: {device_path}"));
    }

    log::info!("正在唤醒摄像头驱动 {device_path} ...");

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

/// 同步打开摄像头. 失败时把错误直接向上抛, 让 `try_start_capture_frames_thread`
/// 把 Result 透传给 `SharedState::rebuild_video` —— 热切摄像头路径上,
/// "设备打不开" 必须在调用方立即可见, 否则会进入"切了但推不出帧"的死状态.
pub(crate) fn open_camera_default(index: CameraIndex) -> anyhow::Result<Camera> {
    log::debug!("Opening camera with index: {index:?}");
    // 构建指定要求的摄像头参数 480p, yuyv格式, 30帧, 如果拿不到这个要求的配置直接报错, 修复了摄像头不配置帧率会报错的问题
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    ensure_camera_ready(&index)?;
    let format_type = RequestedFormatType::Exact(CameraFormat::new(
        Resolution {
            width_x: 640,
            height_y: 480,
        },
        FrameFormat::YUYV,
        30,
    ));
    log::debug!("Supported cameras: {:?}", VideoCapture::list_cameras());
    let query = RequestedFormat::new::<RgbFormat>(format_type);
    Ok(Camera::new(index, query)?)
}

/// 捕获帧循环. `Camera::new` 在这里同步打开 (`open_camera_default`
/// 期间已经过 try_start 的探测, 大概率成功, 但实际打开仍可能因设备
/// 在两次操作之间被拔走而失败).
///
/// `face_detector` 是由 `try_start_capture_frames_thread` 在 30s 超时窗口内
/// 探测到的 detector. None 表示 detector 创建失败/超时, 帧照常出 (但
/// `frame_info.face_info` 全为 default — has_face=false).
fn capture_frames(
    camera_index: CameraIndex,
    bus: FrameCache,
    running: Arc<AtomicBool>,
    resolution: Arc<Mutex<(u32, u32)>>,
    rotate_angle: RotateAngle,
) -> anyhow::Result<()> {
    log::info!("Starting capture thread");
    let mut camera = open_camera_default(camera_index)?;

    // 获取摄像头的信息
    let camera_fmt = camera.camera_format();
    log::info!("camera info: {camera_fmt:?}");
    let width = camera_fmt.width();
    let height = camera_fmt.height();

    // face detector 拿到后才进 frame loop (避免 capture 在构造 detector 时
    // hang 90s+ 没帧出). 这里用 sync_channel + recv_timeout 在 capture
    // thread 内部同步等待, 超时放 None. Box<dyn ...> 在 capture loop 里
    // 每帧 `as_deref_mut` 借出去, 调完即 drop — 不存在跨函数 borrow.
    log::info!("Loading face detector...");
    let model_path = ModelManager::global()
        .get("yolo_face")
        .ok_or_else(|| anyhow::anyhow!("yolo_face model not loaded"))?;
    let (detector_tx, detector_rx) =
        std::sync::mpsc::sync_channel::<Option<Box<dyn FaceDetectorTrait>>>(1);
    thread::spawn(move || {
        let detector = crate::vision::face::create_face_detector(model_path).ok();
        let _ = detector_tx.send(detector);
    });
    let mut face_detector: Option<Box<dyn FaceDetectorTrait>> =
        match detector_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(opt) => {
                if opt.is_some() {
                    log::info!("face detector build: success");
                } else {
                    log::warn!("face detector build: returned None");
                }
                opt
            }
            Err(_) => {
                log::warn!(
                    "face detector build did not complete within 5s; \
                 continuing without face detection (frames still flow)"
                );
                None
            }
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
        "Camera resolution: {width}x{height} (rotate: {rotate_angle:?}, output: {out_width}x{out_height})"
    );

    // 帧率计算
    #[cfg(feature = "fps-counter")]
    let mut fps_counter = FrameRateCounter::new();
    while running.load(Ordering::Relaxed) {
        let frame = match camera.frame() {
            Ok(f) => f,
            Err(e) => {
                log::error!("Camera frame error: {e:?}");
                thread::sleep(Duration::from_millis(100));
                continue;
            }
        };
        let start_time = Instant::now();
        // detector owned Box, take() 移交给 process_frame_by_format, 函数返回
        // 时 drop. 这条路径绕过 NLL "借用跨函数" 死结 — 通过 owned value 而
        // 不是 shared reference, 直接满足编译器. take 后 Option 暂时变 None,
        // 函数返回后 detector_box 字段在 caller 端... 这里 caller 已经是
        // process_frame_by_format 内部, 不需要恢复.
        let Ok(frame_data) = process_frame_by_format(
            frame,
            out_width,
            out_height,
            format,
            rotate_angle,
            width,
            height,
            face_detector.take(),
        ) else {
            // 失败时把 detector 拿回来 (若还有). 这里 face_detector 本来
            // 已经 take, 失败时是 None; 重新装回 detector 需要 take_as_some
            // — 但 take 后就是 None 没东西丢, 跳过恢复.
            continue;
        };
        log::debug!("process used time: {:?}", start_time.elapsed());

        // 计算帧率
        #[cfg(feature = "fps-counter")]
        if let Some(fps) = fps_counter.tick() {
            log::info!("Camera FPS: {:.1}", fps);
        }
        // 通过通道发送帧
        bus.publish(crate::event_bus::BusEvent::CameraVideo(Arc::new(
            frame_data,
        )));
    }

    log::info!("Video capture loop stopped");
    Ok(())
}

/// 处理帧并应用旋转
/// 新流程: 先旋转 -> 后检测 (坐标无需转换)
fn process_and_rotate(
    rgb: Vec<u8>,
    width: u32,
    height: u32,
    face_detector: Option<Box<dyn FaceDetectorTrait>>,
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
/// returns: `FrameData`
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
    face_detector: Option<Box<dyn FaceDetectorTrait>>,
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
            // Box owned 这里, 整个 move 进 process_and_rotate, 函数返回时
            // drop. 借用关系不跨函数 body.
            process_and_rotate(
                rgb_data.into_raw(),
                src_width,
                src_height,
                face_detector,
                rotate_angle,
            )
        }
        _ => {
            anyhow::bail!("Unsupported frame format {format:?}");
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
