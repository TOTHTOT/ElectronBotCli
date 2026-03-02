//! Web 预览服务器实现
//!
//! 使用 Axum 实现 MJPEG 流服务器

use axum::{
    extract::State,
    response::{sse::Event, Html, IntoResponse},
    routing::get,
    Router,
};
use std::convert::Infallible;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
use nokhwa::Camera;
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
use nokhwa::pixel_format::YuyvFormat;
use base64::Engine as _;

/// LCD 帧缓存（使用 std::sync::Mutex 以便从同步代码写入）
type LcdFrameCache = Arc<Mutex<Option<Vec<u8>>>>;
/// 摄像头帧缓存
type CameraFrameCache = Arc<Mutex<Option<Vec<u8>>>>;

/// Web 预览服务器状态
pub struct WebPreviewState {
    /// LCD 帧缓存
    pub lcd_frame: LcdFrameCache,
    /// 摄像头帧缓存
    pub camera_frame: CameraFrameCache,
    /// 服务器运行标志
    pub running: Arc<AtomicBool>,
}

impl WebPreviewState {
    pub fn new() -> Self {
        Self {
            lcd_frame: Arc::new(Mutex::new(None)),
            camera_frame: Arc::new(Mutex::new(None)),
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 获取 LCD 帧缓存的 Arc 句柄（用于从 App 写入）
    pub fn lcd_frame(&self) -> LcdFrameCache {
        self.lcd_frame.clone()
    }
}

/// 创建 HTML 主页
fn create_html_page() -> Html<&'static str> {
    Html(r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <title>ElectronBot Preview</title>
    <style>
        body {
            font-family: Arial, sans-serif;
            margin: 20px;
            background-color: #1a1a1a;
            color: #eee;
        }
        h1 {
            text-align: center;
            color: #00ff88;
        }
        .container {
            display: flex;
            justify-content: center;
            gap: 40px;
            flex-wrap: wrap;
        }
        .preview-box {
            background: #2a2a2a;
            padding: 20px;
            border-radius: 10px;
            text-align: center;
        }
        .preview-box h2 {
            color: #00ccff;
            margin-bottom: 15px;
        }
        img {
            border: 2px solid #444;
            border-radius: 5px;
            max-width: 300px;
        }
        .info {
            margin-top: 20px;
            text-align: center;
            color: #888;
        }
    </style>
</head>
<body>
    <h1>🤖 ElectronBot Preview</h1>
    <div class="container">
        <div class="preview-box">
            <h2>LCD Eyes Animation</h2>
            <img id="lcd-img" src="/lcd" alt="LCD Preview" />
        </div>
        <div class="preview-box">
            <h2>USB Camera</h2>
            <img id="camera-img" src="/camera" alt="Camera Preview" />
        </div>
    </div>
    <div class="info">
        <p>Streams should update automatically</p>
    </div>
    <script>
        // 更新 LCD 图像
        async function updateLcd() {
            try {
                const response = await fetch('/lcd');
                const text = await response.text();
                if (text) {
                    document.getElementById('lcd-img').src = 'data:image/jpeg;base64,' + text;
                }
            } catch(e) {}
            setTimeout(updateLcd, 100);
        }
        // 使用 EventSource 更新摄像头图像
        function updateCamera() {
            try {
                const eventSource = new EventSource('/camera');
                eventSource.onmessage = function(event) {
                    if (event.data) {
                        document.getElementById('camera-img').src = 'data:image/jpeg;base64,' + event.data;
                    }
                };
                eventSource.onerror = function() {
                    eventSource.close();
                    setTimeout(updateCamera, 1000);
                };
            } catch(e) {
                setTimeout(updateCamera, 1000);
            }
        }
        updateLcd();
        updateCamera();
    </script>
</body>
</html>"#)
}

/// 主页路由
async fn index() -> impl IntoResponse {
    create_html_page()
}

/// LCD 流路由 - 返回 Base64 编码的 JPEG
async fn lcd_stream(State(state): State<Arc<WebPreviewState>>) -> impl IntoResponse {
    let lcd_frame = state.lcd_frame.clone();

    // 返回一个流
    let frame = async_stream::stream! {
        loop {
            let frame_data = {
                let guard = lcd_frame.lock().unwrap();
                guard.clone()
            };

            if let Some(frame) = frame_data {
                let jpeg = grayscale_to_jpeg(&frame, 240, 240);
                if !jpeg.is_empty() {
                    // 将 JPEG 转换为 Base64 字符串
                    let encoded = base64::engine::general_purpose::STANDARD.encode(&jpeg);
                    yield Ok::<_, Infallible>(Event::default().data(encoded));
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }
    };

    axum::response::sse::Sse::new(frame)
}

/// 摄像头流路由
async fn camera_stream(State(state): State<Arc<WebPreviewState>>) -> impl IntoResponse {
    let camera_frame = state.camera_frame.clone();

    let frame = async_stream::stream! {
        loop {
            let frame_data = {
                let guard = camera_frame.lock().unwrap();
                guard.clone()
            };

            if let Some(frame) = frame_data {
                let jpeg = bgr_to_jpeg(&frame);
                if jpeg.is_empty() {
                    log::warn!("Empty JPEG, skipping frame");
                } else {
                    log::debug!("JPEG encoded: {} bytes", jpeg.len());
                    // 将 JPEG 转换为 Base64 字符串
                    let encoded = base64::engine::general_purpose::STANDARD.encode(&jpeg);
                    yield Ok::<_, Infallible>(Event::default().data(encoded));
                }
            }

            tokio::time::sleep(tokio::time::Duration::from_millis(33)).await;
        }
    };

    axum::response::sse::Sse::new(frame)
}

/// 将灰度帧转换为 JPEG
fn grayscale_to_jpeg(gray_data: &[u8], width: u32, height: u32) -> Vec<u8> {
    use image::{ImageBuffer, Luma};

    let img: ImageBuffer<Luma<u8>, Vec<u8>> =
        ImageBuffer::from_raw(width, height, gray_data.to_vec()).unwrap_or_else(|| {
            ImageBuffer::from_pixel(width, height, Luma([128]))
        });

    let mut jpeg_bytes = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut jpeg_bytes);

    if let Err(e) = img.write_to(&mut cursor, image::ImageFormat::Jpeg) {
        log::error!("Failed to encode JPEG: {}", e);
        return Vec::new();
    }

    jpeg_bytes
}

/// 将 BGR 帧转换为 JPEG
fn bgr_to_jpeg(bgr_data: &[u8]) -> Vec<u8> {
    use image::{ImageBuffer, Rgb};

    // 调试: 显示数据大小
    let data_len = bgr_data.len();
    log::info!("Frame data size: {} bytes", data_len);

    // 检查是否是 MJPEG 压缩数据 (JPEG 以 FF D8 开头)
    if bgr_data.len() > 2 && bgr_data[0] == 0xFF && bgr_data[1] == 0xD8 {
        log::debug!("Detected MJPEG frame, {} bytes", bgr_data.len());
        return bgr_data.to_vec(); // 直接返回 JPEG 数据
    }

    // 尝试多种常见分辨率 (优先 640x480 速度最快)
    let resolutions = [
        (640, 480),
        (1280, 720),
        (1920, 1080),
        (800, 768),
        (320, 240),
    ];

    // 显示每种分辨率对应的数据大小
    for (width, height) in resolutions.iter() {
        let rgb_size = width * height * 3;
        let yuv_size = width * height * 2;
        log::info!("  {}x{}: RGB24={}, YUYV={}", width, height, rgb_size, yuv_size);
    }

    for (width, height) in resolutions.iter() {
        let expected_len = (*width * *height * 3) as usize;
        if bgr_data.len() == expected_len {
            return bgr_to_jpeg_with_size(bgr_data, *width, *height);
        }
    }

    // 尝试 YUYV 格式 (YUV422, 每像素 2 字节)
    for (width, height) in resolutions.iter() {
        let expected_len = (*width * *height * 2) as usize;
        if bgr_data.len() == expected_len {
            log::info!("Detected YUYV format: {}x{}", width, height);
            // YUYV 转 RGB
            let rgb_data = yuyv_to_rgb(bgr_data, *width, *height);
            return bgr_to_jpeg_with_size(&rgb_data, *width, *height);
        }
    }

    log::warn!("Unknown frame size: {} bytes", bgr_data.len());
    Vec::new()
}

/// YUYV (YUV422) 转 RGB - 标准 USB 摄像头格式
fn yuyv_to_rgb(yuyv_data: &[u8], width: u32, height: u32) -> Vec<u8> {
    use image::{ImageBuffer, Rgb};

    // YUYV 格式: Y U Y V (每4字节表示2个像素)
    let mut img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(width, height);

    for y in 0..height {
        for x in (0..width).step_by(2) {
            let idx = ((y * width + x) * 2) as usize;
            if idx + 3 < yuyv_data.len() {
                // YUYV: Y 在前, U V 在后
                // 尝试交换 U 和 V (Y V Y U)
                let y0 = yuyv_data[idx] as f32;
                let v = yuyv_data[idx + 1] as f32;  // 原来这里是 U
                let y1 = yuyv_data[idx + 2] as f32;
                let u = yuyv_data[idx + 3] as f32;  // 原来这里是 V

                // YUV 到 RGB (BT.601)
                let r0 = (y0 + 1.402 * (v - 128.0)).clamp(0.0, 255.0) as u8;
                let g0 = (y0 - 0.344136 * (u - 128.0) - 0.714136 * (v - 128.0)).clamp(0.0, 255.0) as u8;
                let b0 = (y0 + 1.772 * (u - 128.0)).clamp(0.0, 255.0) as u8;

                img.put_pixel(x, y, Rgb([r0, g0, b0]));

                if x + 1 < width {
                    let r1 = (y1 + 1.402 * (v - 128.0)).clamp(0.0, 255.0) as u8;
                    let g1 = (y1 - 0.344136 * (u - 128.0) - 0.714136 * (v - 128.0)).clamp(0.0, 255.0) as u8;
                    let b1 = (y1 + 1.772 * (u - 128.0)).clamp(0.0, 255.0) as u8;
                    img.put_pixel(x + 1, y, Rgb([r1, g1, b1]));
                }
            }
        }
    }

    img.into_raw()
}

fn bgr_to_jpeg_with_size(bgr_data: &[u8], width: u32, height: u32) -> Vec<u8> {
    use image::{ImageBuffer, Rgb};

    // 转换为 RGB
    let rgb_data: Vec<u8> = bgr_data
        .chunks(3)
        .flat_map(|chunk| vec![chunk[2], chunk[1], chunk[0]])
        .collect();

    let img: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_raw(width, height, rgb_data).unwrap_or_else(|| {
            ImageBuffer::from_pixel(width, height, Rgb([128, 128, 128]))
        });

    let mut jpeg_bytes = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut jpeg_bytes);

    if let Err(e) = img.write_to(&mut cursor, image::ImageFormat::Jpeg) {
        log::error!("Failed to encode JPEG: {}", e);
        return Vec::new();
    }

    jpeg_bytes
}

/// Web 预览服务器
#[derive(Clone)]
pub struct WebPreview {
    state: Arc<WebPreviewState>,
    port: u16,
}

impl WebPreview {
    /// 创建新的 Web 预览服务器
    pub fn new(port: u16) -> Self {
        Self {
            state: Arc::new(WebPreviewState::new()),
            port,
        }
    }

    /// 获取状态句柄（用于发送 LCD 帧）
    pub fn state(&self) -> Arc<WebPreviewState> {
        self.state.clone()
    }

    /// 获取 LCD 帧缓存句柄（用于 App 写入帧数据）
    pub fn lcd_frame_cache(&self) -> LcdFrameCache {
        self.state.lcd_frame()
    }

    /// 启动服务器（阻塞）
    pub async fn run(self) {
        let addr = format!("0.0.0.0:{}", self.port);
        log::info!("Starting web preview server at http://{}", addr);

        let app = Router::new()
            .route("/", get(index))
            .route("/lcd", get(lcd_stream))
            .route("/camera", get(camera_stream))
            .with_state(self.state.clone());

        self.state.running.store(true, Ordering::Relaxed);

        let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
        log::info!("Web server listening on {}", addr);

        // 启动摄像头捕获任务（使用 std::thread 因为 Camera 不是 Send）
        let camera_state = self.state.clone();
        std::thread::spawn(move || {
            Self::camera_capture_task_blocking(camera_state);
        });

        // 运行服务器
        axum::serve(listener, app).await.unwrap();
    }

    /// 摄像头捕获任务（阻塞式，在独立线程中运行）
    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    fn camera_capture_task_blocking(state: Arc<WebPreviewState>) {
        // 等待服务器启动
        std::thread::sleep(std::time::Duration::from_millis(500));

        // 尝试初始化摄像头
        let mut camera = match Self::init_camera() {
            Some(cam) => cam,
            None => {
                log::warn!("No camera available, camera stream will be unavailable");
                return;
            }
        };

        log::info!("Camera initialized, starting capture loop");

        let mut frame_count = 0;

        loop {
            if !state.running.load(Ordering::Relaxed) {
                break;
            }

            // 跳帧处理: 每3帧处理1帧，大幅减少CPU负担
            frame_count += 1;
            if frame_count % 3 != 0 {
                std::thread::sleep(std::time::Duration::from_millis(5));
                continue;
            }

            match camera.frame() {
                Ok(frame) => {
                    let buffer = frame.buffer().to_vec();
                    log::info!("Camera frame: {} bytes", buffer.len());
                    let mut guard = state.camera_frame.lock().unwrap();
                    *guard = Some(buffer);
                }
                Err(e) => {
                    log::error!("Camera frame error: {:?}", e);
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
        }

        log::info!("Camera capture task stopped");
    }

    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    fn camera_capture_task_blocking(_state: Arc<WebPreviewState>) {
        log::warn!("Camera not supported on this platform");
    }

    /// 初始化摄像头
    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    fn init_camera() -> Option<Camera> {
        log::info!("Trying to initialize camera...");

        // 尝试多种分辨率 (优先 1080p)
        let resolutions = [(640, 480), (1280, 720), (1920, 1080), (800, 768), (320, 240)];

        // 尝试 YUYV 格式
        log::info!("Trying YUYV format...");
        for (width, height) in resolutions.iter() {
            log::info!("Trying YUYV {}x{}", width, height);

            let query = nokhwa::utils::RequestedFormat::new::<YuyvFormat>(
                nokhwa::utils::RequestedFormatType::HighestResolution(
                    nokhwa::utils::Resolution::new(*width, *height)
                ),
            );

            match Camera::new(nokhwa::utils::CameraIndex::Index(0), query) {
                Ok(cam) => {
                    log::info!("Camera opened with YUYV {}x{}", width, height);
                    return Some(cam);
                }
                Err(e) => {
                    log::warn!("Failed to open camera with YUYV {}x{}: {:?}", width, height, e);
                }
            }
        }

        log::error!("Could not open camera with any resolution");
        None
    }

    /// 停止服务器
    pub fn stop(&self) {
        self.state.running.store(false, Ordering::Relaxed);
    }
}
