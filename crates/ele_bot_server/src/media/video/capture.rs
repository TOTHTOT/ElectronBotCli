//! 视频模块 - 摄像头捕获

use crate::media::video::process::{
    fast_yuyv_to_rgb, fast_yuyv_to_rgb_rot270, process_frame, rga_yuyv_to_rgb, rotate_by_angle,
    RotateAngle,
};
use crate::media::video::types::{CameraFormat as LocalCameraFormat, FrameCache, FrameInfo};
use crate::model_manager::ModelManager;
use crate::vision::face::{FaceDetectionResult, FaceDetectorTrait};
use bytes::Bytes;
use image::RgbImage;
use nokhwa::pixel_format::RgbFormat;
use nokhwa::utils::{
    ApiBackend, CameraFormat, CameraIndex, CameraInfo, FrameFormat, RequestedFormat,
    RequestedFormatType, Resolution,
};
// 仅 aarch64 的曝光控制调试代码用
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
use nokhwa::utils::{ControlValueSetter, KnownCameraControl};
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
        let list = query(ApiBackend::Auto)?;
        // Linux 上 SoC 硬编解码器也会注册 v4l2 节点 (rk3566 BSP 命名为
        // /dev/video-encN / /dev/video-decN), 它们不是摄像头, 不能出现在
        // 下拉列表里 — 按节点名过滤掉. nokhwa v4l 枚举把节点路径放在
        // description ("Video4Linux Device @ /dev/videoN"), misc 为空,
        // 所以两个字段都要看
        #[cfg(target_os = "linux")]
        let list = list
            .into_iter()
            .filter(|ci| {
                let path = format!("{} {}", ci.misc(), ci.description());
                !(path.contains("video-enc") || path.contains("video-dec"))
            })
            .collect();
        Ok(list)
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
    let mut camera = Camera::new(index, query)?;
    log::debug!("select camera controls: {:#?}", camera.camera_controls()?);
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))] // 调试摄像头用的
    {
        // 显式设为 Aperture Priority (3) 保持自动曝光: 白天亮度自适应好,
        // 代价是弱光下曝光时间拉长, 帧率掉到 5-8fps (manual 模式可恒 30fps,
        // 但暗处画面太黑, 当前选择保画质).
        camera.set_camera_control(
            KnownCameraControl::Other(10094849),
            ControlValueSetter::Integer(3),
        )?;
        // 手动曝光参考 (夜间恒 30fps 用):
        // camera.set_camera_control(
        //     KnownCameraControl::Other(10094849),
        //     ControlValueSetter::Integer(1),
        // )?;
        // camera.set_camera_control(
        //     KnownCameraControl::Other(10094850),
        //     ControlValueSetter::Integer(625),
        // )?;
    }
    // nokhwa 0.10: `Camera::new` 只初始化设备, 必须显式 `open_stream`
    // 才会真正开流 (v4l 上不开流读帧直接报 "Stream Not Started")
    camera.open_stream()?;
    Ok(camera)
}

/// 捕获帧循环. `Camera::new` 在这里同步打开 (`open_camera_default`
/// 期间已经过 try_start 的探测, 大概率成功, 但实际打开仍可能因设备
/// 在两次操作之间被拔走而失败).
///
/// `face_detector` 构造失败/超时时为 None, 帧照常出 (但
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
    let camera_fmt = camera.camera_format();
    log::info!("camera info: {camera_fmt:?}");
    let (width, height) = (camera_fmt.width(), camera_fmt.height());
    let format = camera_fmt.format();

    let mut face_detector = build_face_detector()?;

    // 如果需要旋转 90 或 270 度，宽高交换
    let (out_width, out_height) = if rotate_angle.needs_swap() {
        (height, width)
    } else {
        (width, height)
    };
    if let Ok(mut guard) = resolution.lock() {
        *guard = (out_width, out_height);
    }
    log::info!(
        "Camera resolution: {width}x{height} (rotate: {rotate_angle:?}, output: {out_width}x{out_height})"
    );

    let ctx = FramePipelineCtx {
        out_width,
        out_height,
        format,
        rotate_angle,
        src_width: width,
        src_height: height,
    };
    let mut stats = LoopStats::new();
    // 隔帧检测: RKNN 推理 ~19ms, 每帧都检测会把 30fps 的 33ms 帧预算
    // 占满 (~30ms/帧), 稍有抖动就积帧. 每 2 帧检测一次 + 复用上次结果
    // 画框, 均值降到 ~19ms, 框刷新 15Hz 肉眼无感.
    const DETECT_EVERY_N: u64 = 2;
    let mut frame_no: u64 = 0;
    let mut last_face = FaceDetectionResult::default();
    while running.load(Ordering::Relaxed) {
        let Some(frame) = read_next_frame(&mut camera) else {
            continue;
        };
        let start_time = Instant::now();
        let run_detect = frame_no.is_multiple_of(DETECT_EVERY_N);
        frame_no += 1;
        // 借出 &mut 给 process_frame_by_format, 函数返回借用即结束,
        // 下一帧可以继续用同一个 detector (之前用 take() 传 owned Box,
        // detector 在第一帧就被 drop, 人脸检测静默失效; 参数上的
        // `+ 'static` 是让这里能内联借用而不触发 E0597 的关键).
        let detector_arg = if run_detect {
            face_detector.as_deref_mut()
        } else {
            None
        };
        let Ok(frame_info) = process_frame_by_format(frame, &ctx, detector_arg, Some(&last_face))
        else {
            continue;
        };
        if run_detect {
            last_face = frame_info.face_info.clone();
        }
        log::debug!(
            "process frame and face detector used time: {:?}",
            start_time.elapsed()
        );

        bus.publish(crate::event_bus::BusEvent::CameraVideo(Arc::new(
            frame_info,
        )));
        stats.tick();
    }

    log::info!("Video capture loop stopped");
    Ok(())
}

/// 取下一帧. 读失败时打日志 + 睡 100ms 避让, 返回 None 让循环继续.
fn read_next_frame(camera: &mut Camera) -> Option<nokhwa::Buffer> {
    let read_start = Instant::now();
    let frame = match camera.frame() {
        Ok(f) => f,
        Err(e) => {
            log::error!("Camera frame error: {e:?}");
            thread::sleep(Duration::from_millis(100));
            return None;
        }
    };
    // 含等待摄像头出帧的阻塞时间; 若接近帧间隔 (33ms) 说明处理没积压,
    // 接近 0 说明摄像头队列里攒着帧等我们消费 (处理是瓶颈)
    log::debug!("frame read used time: {:?}", read_start.elapsed());
    Some(frame)
}

/// 构造人脸检测器, 5s 超时. 模型文件缺失返回 Err (终止 capture);
/// 构造超时/失败返回 Ok(None) — 帧照常出, 只是 face_info 恒为 default.
///
/// 用 sync_channel + recv_timeout 在 capture thread 内部同步等待是刻意的:
/// detector 构造可能 hang (ort 2.0 RC 创建 session 曾 hang 90s+),
/// 超时就放弃检测, 不阻塞帧流出.
fn build_face_detector() -> anyhow::Result<Option<Box<dyn FaceDetectorTrait>>> {
    log::info!("Loading face detector...");
    // aarch64 (rk3566) 用 RKNN RetinaFace, 其他平台用 ONNX yolo_face —
    // 与 model_manager 的注册 key 保持一致
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    let face_model_key = "retinaface_rknn";
    #[cfg(not(all(target_os = "linux", target_arch = "aarch64")))]
    let face_model_key = "yolo_face";
    let model_path = ModelManager::global()
        .get(face_model_key)
        .ok_or_else(|| anyhow::anyhow!("{face_model_key} model not loaded"))?;

    let (tx, rx) = std::sync::mpsc::sync_channel::<Option<Box<dyn FaceDetectorTrait>>>(1);
    thread::spawn(move || {
        let detector = crate::vision::face::create_face_detector(model_path).ok();
        let _ = tx.send(detector);
    });
    let detector = match rx.recv_timeout(Duration::from_secs(5)) {
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
    Ok(detector)
}

/// 循环帧率统计: 每 5s 打一条 info 日志, 用于耗时分析.
struct LoopStats {
    frames: u32,
    start: Instant,
}

impl LoopStats {
    fn new() -> Self {
        Self {
            frames: 0,
            start: Instant::now(),
        }
    }

    fn tick(&mut self) {
        self.frames += 1;
        let elapsed = self.start.elapsed();
        if elapsed >= Duration::from_secs(5) {
            log::info!(
                "capture stats: {} frames in {:.1}s = {:.1} FPS",
                self.frames,
                elapsed.as_secs_f64(),
                f64::from(self.frames) / elapsed.as_secs_f64()
            );
            self.frames = 0;
            self.start = Instant::now();
        }
    }
}

/// 处理帧并应用旋转
/// 新流程: 先旋转 -> 后检测 (坐标无需转换)
fn process_and_rotate(
    rgb: Vec<u8>,
    width: u32,
    height: u32,
    face_detector: Option<&mut (dyn FaceDetectorTrait + 'static)>,
    cached_face: Option<&FaceDetectionResult>,
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
    let processed = process_frame(
        rotated,
        new_width,
        new_height,
        face_detector,
        cached_face,
        None,
    )?;
    log::debug!("process_frame used time: {:?}", detect_start.elapsed());
    Ok(processed)
}

/// 帧处理管线的不变上下文: 进循环前构建一次, 每帧按引用传入.
/// 把 `process_frame_by_format` 的 6 个不变参数收拢成一个结构,
/// 签名从 8 参降到 3 参, 也消除了参数顺序写错的可能.
struct FramePipelineCtx {
    /// 输出图像的宽 (旋转后)
    out_width: u32,
    /// 输出图像的高 (旋转后)
    out_height: u32,
    /// 摄像头协商出的帧格式
    format: FrameFormat,
    rotate_angle: RotateAngle,
    /// 摄像头原始宽
    src_width: u32,
    /// 摄像头原始高
    src_height: u32,
}

/// 根据帧格式处理数据: 解码 -> 旋转 -> 人脸检测/画框 -> `FrameInfo`.
///
/// YUYV 优先走硬件 RGA (`process_yuyv_rga`), 不可用回退软件
/// (`process_yuv_software`); MJPEG 走 image crate 软件解码.
fn process_frame_by_format(
    frame: nokhwa::Buffer,
    ctx: &FramePipelineCtx,
    mut face_detector: Option<&mut (dyn FaceDetectorTrait + 'static)>,
    cached_face: Option<&FaceDetectionResult>,
) -> anyhow::Result<FrameInfo> {
    match ctx.format {
        FrameFormat::YUYV | FrameFormat::NV12 => {
            log::debug!(
                "Frame: {}x{}, len: {}, YUV, decoding...",
                ctx.out_width,
                ctx.out_height,
                frame.buffer().len()
            );
            let decode_start = Instant::now();
            if let Some(res) = process_yuyv_rga(
                &frame,
                ctx,
                face_detector.as_deref_mut(),
                cached_face,
                decode_start,
            ) {
                return res;
            }
            process_yuv_software(frame, ctx, face_detector, cached_face, decode_start)
        }
        FrameFormat::MJPEG => {
            // nokhwa 的 decode_image::<RgbFormat>() 不会解 MJPEG 压缩流;
            // MJPEG 摄像头 (常见 USB 高分辨率设备) 给的是完整 JPEG 字节流,
            // 走 image crate 的 JPEG decoder. PC 上 640x480 单帧解码 < 5ms,
            // 不需要切硬件; RK3566 上若帧率顶不住再考虑 RGA / hardware jpeg.
            log::debug!(
                "Frame: {}x{}, len: {}, MJPEG, decoding...",
                ctx.out_width,
                ctx.out_height,
                frame.buffer().len()
            );
            let Some(rgb_data) = decode_jpeg_to_rgb(frame.buffer()) else {
                anyhow::bail!("failed to decode MJPG");
            };
            process_and_rotate(
                rgb_data,
                ctx.src_width,
                ctx.src_height,
                face_detector,
                cached_face,
                ctx.rotate_angle,
            )
        }
        _ => {
            anyhow::bail!("Unsupported frame format {:?}", ctx.format);
        }
    }
}

/// YUYV 硬件路径: RGA CSC+旋转单 pass (~2.2ms, librga 1.10.6+),
/// 检测帧顺带第二个 pass 直接产出 320x320 检测输入 (~1.2ms), 省掉
/// 检测器内部 "全帧图再 resize" (~2.9ms).
///
/// 旧注: 曾尝试 RGA CSC 输出全绿 — 根因是设备系统自带 librga
/// rga_api 1.3.2 太老 (彩条隔离测试证实), 现用 assets/lib 随包
/// 部署的官方 1.10.6, $ORIGIN rpath 加载, 已验证 8 色彩条精确.
///
/// RGA 不可用/失败返回 None, 调用方回退 `process_yuv_software`.
fn process_yuyv_rga(
    frame: &nokhwa::Buffer,
    ctx: &FramePipelineCtx,
    face_detector: Option<&mut (dyn FaceDetectorTrait + 'static)>,
    cached_face: Option<&FaceDetectionResult>,
    decode_start: Instant,
) -> Option<anyhow::Result<FrameInfo>> {
    if ctx.format != FrameFormat::YUYV {
        return None;
    }
    let rot = rga_yuyv_to_rgb(
        frame.buffer(),
        ctx.src_width,
        ctx.src_height,
        ctx.out_width,
        ctx.out_height,
        ctx.rotate_angle,
    )?;
    log::debug!(
        "rga yuyv csc+rotate used time: {:?}",
        decode_start.elapsed()
    );

    let det_input = face_detector
        .as_ref()
        .and_then(|d| d.input_size())
        .and_then(|(dw, dh)| {
            rga_yuyv_to_rgb(
                frame.buffer(),
                ctx.src_width,
                ctx.src_height,
                dw,
                dh,
                ctx.rotate_angle,
            )
            .map(|v| (v, dw, dh))
        });
    if det_input.is_some() {
        log::debug!(
            "rga yuyv csc+rotate+resize (det input) total: {:?}",
            decode_start.elapsed()
        );
    }

    let detect_start = Instant::now();
    let processed = process_frame(
        rot,
        ctx.out_width,
        ctx.out_height,
        face_detector,
        cached_face,
        det_input.as_ref().map(|(v, w, h)| (v.as_slice(), *w, *h)),
    );
    log::debug!("process_frame used time: {:?}", detect_start.elapsed());
    Some(processed)
}

/// YUV 软件兜底: YUYV 手写 LUT 解码 (rot270 走解码+旋转融合单 pass
/// ~6.5ms), NV12 走 nokhwa decode_image (~10ms), 之后统一
/// `process_and_rotate`.
fn process_yuv_software(
    frame: nokhwa::Buffer,
    ctx: &FramePipelineCtx,
    face_detector: Option<&mut (dyn FaceDetectorTrait + 'static)>,
    cached_face: Option<&FaceDetectionResult>,
    decode_start: Instant,
) -> anyhow::Result<FrameInfo> {
    if ctx.format == FrameFormat::YUYV && ctx.rotate_angle == RotateAngle::Rotate270 {
        // 解码 + 旋转 270° 融合单 pass, 跳过独立的 rotate 步骤.
        let rotated = fast_yuyv_to_rgb_rot270(frame.buffer(), ctx.src_width, ctx.src_height);
        log::debug!(
            "yuyv decode+rotate270 used time: {:?}",
            decode_start.elapsed()
        );
        let detect_start = Instant::now();
        let processed = process_frame(
            rotated,
            ctx.out_width,
            ctx.out_height,
            face_detector,
            cached_face,
            None,
        )?;
        log::debug!("process_frame used time: {:?}", detect_start.elapsed());
        return Ok(processed);
    }
    let rgb_data = if ctx.format == FrameFormat::YUYV {
        fast_yuyv_to_rgb(frame.buffer(), ctx.src_width, ctx.src_height)
    } else {
        let Ok(d) = frame.decode_image::<RgbFormat>() else {
            anyhow::bail!("failed to decode image");
        };
        d.into_raw()
    };
    log::debug!("yuv decode used time: {:?}", decode_start.elapsed());
    process_and_rotate(
        rgb_data,
        ctx.src_width,
        ctx.src_height,
        face_detector,
        cached_face,
        ctx.rotate_angle,
    )
}

/// 解码 JPEG / MJPEG 字节流为 RGB 原始数据.
///
/// 给摄像头送 MJPG 帧时, nokhwa 的 `frame.decode_image::<RgbFormat>()` 不会
/// 解压缩, 缓冲区里是完整 JPEG 字节流. 这里用 `image` crate 走软件 JPEG
/// 解码, 转成 `RGB8` 后丢给 `process_and_rotate` 后续旋转 / 推流.
/// 当前在 `process_frame_by_format` 的 `FrameFormat::MJPEG` 分支调用.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_camera_index_integer() {
        assert!(matches!(parse_camera_index("1"), CameraIndex::Index(1)));
        assert!(matches!(parse_camera_index("0"), CameraIndex::Index(0)));
    }

    #[test]
    fn parse_camera_index_empty_defaults_to_zero() {
        assert!(matches!(parse_camera_index(""), CameraIndex::Index(0)));
    }

    #[test]
    fn parse_camera_index_non_integer_goes_string() {
        match parse_camera_index("/dev/video0") {
            CameraIndex::String(s) => assert_eq!(s, "/dev/video0"),
            other => panic!("expected String index, got {other:?}"),
        }
    }

    #[test]
    fn camera_info_to_dto_empty_human_name_falls_back() {
        // human_name 和 description 都为空 -> display 退化成 `Camera {id} (id={id})`
        let ci = CameraInfo::new("", "", "/dev/video0", CameraIndex::Index(0));
        let dto = camera_info_to_dto(&ci);
        assert_eq!(dto.id, "0");
        assert_eq!(dto.display, "Camera 0 (id=0)");
    }

    #[test]
    fn camera_info_to_dto_dedups_driver_and_name() {
        // description 缺失时 driver 也退到 `Camera {id}`, 与 name 相同,
        // display 应合并为单段而不是 `Camera 0 Camera 0 (id=0)`.
        let ci = CameraInfo::new("", "", "/dev/video0", CameraIndex::Index(1));
        let dto = camera_info_to_dto(&ci);
        assert_eq!(dto.display, "Camera 1 (id=1)");
    }

    #[test]
    fn camera_info_to_dto_full_fields() {
        let ci = CameraInfo::new(
            "USB 2.0 PC Cam",
            "V4L2 Camera",
            "/dev/video0",
            CameraIndex::Index(0),
        );
        let dto = camera_info_to_dto(&ci);
        assert_eq!(dto.name, "USB 2.0 PC Cam");
        assert_eq!(dto.display, "V4L2 Camera USB 2.0 PC Cam (id=0)");
    }
}
