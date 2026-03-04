//! Web 预览服务器实现
//!
//! 使用 Axum 实现 MJPEG 流服务器

use axum::{
    extract::State,
    response::{sse::Event, Html, IntoResponse},
    routing::get,
    Router,
};
use bytes::Bytes;
use std::convert::Infallible;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::media::video::types::FrameCache;
use base64::Engine as _;

/// LCD 帧缓存
type LcdFrameCache = Arc<Mutex<Option<Vec<u8>>>>;

/// Web 预览服务器状态
pub struct WebPreviewState {
    /// LCD 帧缓存
    pub lcd_frame: LcdFrameCache,
    /// 摄像头帧缓存
    pub camera_frame: FrameCache,
    /// 服务器运行标志
    pub running: Arc<AtomicBool>,
    /// 摄像头分辨率 (width, height)
    pub camera_resolution: Arc<Mutex<(u32, u32)>>,
}

impl WebPreviewState {
    pub fn new(camera_resolution: Arc<Mutex<(u32, u32)>>, camera_frame: FrameCache) -> Self {
        Self {
            lcd_frame: Arc::new(Mutex::new(None)),
            camera_frame,
            running: Arc::new(AtomicBool::new(false)),
            camera_resolution,
        }
    }

    /// 获取 LCD 帧缓存的 Arc 句柄
    pub fn lcd_frame(&self) -> LcdFrameCache {
        self.lcd_frame.clone()
    }
}

/// 创建 HTML 主页
fn create_html_page() -> Html<&'static str> {
    Html(
        r#"<!DOCTYPE html>
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
</html>"#,
    )
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
    // 获取已知分辨率
    let resolution = state.camera_resolution.clone();

    let frame = async_stream::stream! {
        loop {
            let frame_data = {
                let guard = camera_frame.lock().unwrap();
                guard.clone()
            };

            if let Some(frame) = frame_data {
                // 根据数据类型获取 JPEG
                let jpeg = if let Some(jpeg_data) = frame.as_jpeg() {
                    // 已经是 JPEG（MJPEG 格式），直接使用
                    jpeg_data.clone()
                } else if let Some(bgr_data) = frame.as_raw_bgr() {
                    // 需要编码为 JPEG，使用已知分辨率
                    let (width, height) = *resolution.lock().unwrap();
                    bgr_to_jpeg_with_size(bgr_data, width, height)
                } else {
                    Bytes::new()
                };

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
fn grayscale_to_jpeg(gray_data: &[u8], width: u32, height: u32) -> Bytes {
    use image::{ImageBuffer, Luma};

    let img: ImageBuffer<Luma<u8>, Vec<u8>> =
        ImageBuffer::from_raw(width, height, gray_data.to_vec())
            .unwrap_or_else(|| ImageBuffer::from_pixel(width, height, Luma([128])));

    let mut jpeg_bytes = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut jpeg_bytes);

    if let Err(e) = img.write_to(&mut cursor, image::ImageFormat::Jpeg) {
        log::error!("Failed to encode JPEG: {}", e);
        return Bytes::new();
    }

    Bytes::from(jpeg_bytes)
}

fn bgr_to_jpeg_with_size(bgr_data: &Bytes, width: u32, height: u32) -> Bytes {
    use image::{ImageBuffer, Rgb};

    // 转换为 RGB
    let rgb_data: Vec<u8> = bgr_data
        .chunks(3)
        .flat_map(|chunk| vec![chunk[2], chunk[1], chunk[0]])
        .collect();

    let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_raw(width, height, rgb_data)
        .unwrap_or_else(|| ImageBuffer::from_pixel(width, height, Rgb([128, 128, 128])));

    let mut jpeg_bytes = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut jpeg_bytes);

    if let Err(e) = img.write_to(&mut cursor, image::ImageFormat::Jpeg) {
        log::error!("Failed to encode JPEG: {}", e);
        return Bytes::new();
    }

    Bytes::from(jpeg_bytes)
}

/// Web 预览服务器
#[derive(Clone)]
pub struct WebPreview {
    state: Arc<WebPreviewState>,
    port: u16,
}

#[allow(dead_code)]
impl WebPreview {
    /// 创建新的 Web 预览服务器
    ///
    /// # Arguments
    /// * `port` - 服务器端口
    /// * `camera_frame` - 摄像头帧缓存（由 VideoCapture 提供）
    /// * `camera_resolution` - 摄像头分辨率 (width, height)
    pub fn new(
        port: u16,
        camera_frame: FrameCache,
        camera_resolution: Arc<Mutex<(u32, u32)>>,
    ) -> Self {
        let state = Arc::new(WebPreviewState::new(camera_resolution, camera_frame));

        Self { state, port }
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
        log::info!("Starting web preview server at https://{}", addr);

        let app = Router::new()
            .route("/", get(index))
            .route("/lcd", get(lcd_stream))
            .route("/camera", get(camera_stream))
            .with_state(self.state.clone());

        self.state.running.store(true, Ordering::Relaxed);

        let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
        log::info!("Web server listening on {}", addr);

        // 运行服务器
        axum::serve(listener, app).await.unwrap();
    }

    /// 停止服务器
    pub fn stop(&self) {
        self.state.running.store(false, Ordering::Relaxed);
    }
}
