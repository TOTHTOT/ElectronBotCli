pub mod config;
pub mod face_tracker;
pub mod menu;

use crate::app::face_tracker::{calculate_body_adjustment, smooth_adjustment};
use crate::llm::QwenLlm;
use crate::media::video::types::FrameInfo;
use crate::media::video::VideoCapture;
use crate::media::voice::play_beep;
use crate::media::voice::VoiceManager;
use crate::model_manager::ModelManager;
use crate::robot::{self, CommState, DisplayMode, Joint, JointConfig, Lcd};
use crate::ui::pages::llm_test::LlmTestState;
use crate::vision::face::create_face_detector;
use crate::web::WebPreview;
use boteyes::Mood;
use electron_bot::{FRAME_HEIGHT, FRAME_WIDTH};
pub use menu::*;
use ratatui::widgets::ListState;
use std::default::Default;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

// 全局模型实例 - 只读初始化，无锁竞争

pub type BotRecvType = (Vec<u8>, JointConfig);

/// LCD 帧缓存类型
type LcdFrameCache = Option<Arc<Mutex<Option<Vec<u8>>>>>;

/// UI 状态 - 菜单、导航、设置等
pub(crate) struct UiState {
    pub menu_state: ListState,
    pub selected_menu: MenuItem,
    pub running: bool,
    pub left_focused: bool,
    pub in_settings: bool,
    pub settings_selected: usize,
    pub in_edit_settings_mode: bool,
    pub edit_buffer: String,
    pub in_llm_test_mode: bool,
}

/// AI 状态 - LLM、语音、情感识别
pub(crate) struct AiState {
    pub voice_manager: VoiceManager,
    voice_result_rx: Option<mpsc::Receiver<Mood>>,
    pub is_processing: Arc<AtomicBool>,
    pub text_tx: Sender<String>,
    pub llm_test_state: LlmTestState,
}

/// 通信状态
pub(crate) struct Comm {
    pub state: Option<CommState>,
    pub thread: Option<std::thread::JoinHandle<()>>,
    pub tx: Option<SyncSender<BotRecvType>>,
}

/// 视频/摄像头状态
pub(crate) struct Video {
    /// LCD 帧缓存（用于 Web 预览）
    lcd_frame_cache: LcdFrameCache,
    /// 视频捕获器, 这里需要持有他免得生命周期结束被释放了
    _video_capture: VideoCapture,
    /// 帧通道接收端，用于获取人脸位置信息
    frame_rx: Option<broadcast::Receiver<FrameInfo>>,
}

/// 主应用
#[allow(dead_code)]
pub struct App {
    // UI 状态
    pub ui: UiState,

    // 硬件
    pub joint: Arc<Joint>,
    pub in_servo_mode: bool,
    pub lcd: Lcd,

    // 弹窗
    pub popup: Popup,

    // 配置
    pub config: config::AppConfig,

    // AI 状态
    pub ai: AiState,

    // 通信
    pub comm: Comm,

    // 视频/摄像头
    pub video: Video,

    // 人脸追踪
    pub face_tracking_enabled: bool,
    last_face_adjustment: i32,
}

#[allow(dead_code)]
impl App {
    pub fn new() -> anyhow::Result<Self> {
        let lcd = Lcd::new();
        let config = config::AppConfig::load();

        // 只初始化 ModelManager, 检查/下载模型元数据
        let _ = ModelManager::global();

        // 在创建 LLM 线程前直接初始化 LLM
        let llm = Self::init_llm(ModelManager::global())?;
        let (text_tx, result_rx) = Self::spawn_llm_thread(llm)?;

        // 初始化视频捕获
        let (video_capture, lcd_frame_cache, frame_rx) = Self::init_video(&config)?;

        // 初始化语音管理器
        let voice_manager = Self::init_voice_manager(&config)?;

        log::info!("init app successfully");

        let mut menu_state = ListState::default();
        menu_state.select(Some(0));

        Ok(Self {
            ui: UiState {
                menu_state,
                selected_menu: MenuItem::DeviceStatus,
                running: true,
                left_focused: true,
                in_settings: false,
                settings_selected: 0,
                in_edit_settings_mode: false,
                in_llm_test_mode: false,
                edit_buffer: String::new(),
            },
            joint: Arc::new(Joint::new()),
            in_servo_mode: false,
            lcd,
            popup: Popup::new(),
            config,
            ai: AiState {
                voice_manager,
                voice_result_rx: Some(result_rx),
                is_processing: Arc::new(AtomicBool::new(false)),
                text_tx,
                llm_test_state: LlmTestState::default(),
            },
            comm: Comm {
                state: None,
                thread: None,
                tx: None,
            },
            video: Video {
                lcd_frame_cache,
                _video_capture: video_capture,
                frame_rx: Some(frame_rx),
            },
            face_tracking_enabled: false,
            last_face_adjustment: 0,
        })
    }

    /// 初始化 LLM
    fn init_llm(mm: &ModelManager) -> anyhow::Result<Arc<Mutex<QwenLlm>>> {
        log::info!("start load llm");
        let Some(qw_tokenizer_path) = mm.get("tokenizer") else {
            anyhow::bail!("tokenizer not found");
        };
        let Some(qw_path) = mm.get("qwen") else {
            anyhow::bail!("qwen not found");
        };
        let mut llm = QwenLlm::load(qw_path);
        llm.load_tokenizer(qw_tokenizer_path)?;
        llm.preload()?;
        Ok(Arc::new(Mutex::new(llm)))
    }

    /// 初始化人脸检测器
    fn init_face_detector(
        mm: &ModelManager,
    ) -> anyhow::Result<Box<dyn crate::vision::face::FaceDetectorTrait>> {
        log::info!("start load face detector");
        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        let Some(face_detect) = mm.get("retinaface_rknn") else {
            anyhow::bail!("retinaface_rknn not found");
        };
        #[cfg(not(all(target_os = "linux", target_arch = "aarch64")))]
        let Some(face_detect) = mm.get("yolo_face") else {
            anyhow::bail!("yolo_face not found");
        };
        create_face_detector(face_detect)
    }

    /// 初始化语音管理器
    fn init_voice_manager(config: &config::AppConfig) -> anyhow::Result<VoiceManager> {
        log::info!("start load voice manager");
        let mm = ModelManager::global();
        if let (Some(sense_voice_path), Some(silero_vad_path), Some(tokens_path)) = (
            mm.get("sense_voice"),
            mm.get("silero_vad"),
            mm.get("sense_voice_tokens"),
        ) {
            VoiceManager::new(
                sense_voice_path,
                silero_vad_path,
                tokens_path,
                &config.speech_name,
            )
        } else {
            anyhow::bail!("Voice model not available");
        }
    }

    /// 启动 LLM 处理线程
    fn spawn_llm_thread(
        llm: Arc<Mutex<QwenLlm>>,
    ) -> anyhow::Result<(Sender<String>, mpsc::Receiver<Mood>)> {
        let (text_tx, text_rx) = mpsc::channel::<String>();
        let (result_tx, result_rx) = mpsc::channel();

        let is_processing = Arc::new(AtomicBool::new(false));
        let is_processing_clone = is_processing.clone();

        std::thread::spawn(move || {
            Self::llm_analysis_thread(llm, text_rx, result_tx, is_processing_clone);
        });

        Ok((text_tx, result_rx))
    }

    /// 初始化视频捕获
    fn init_video(
        config: &config::AppConfig,
    ) -> anyhow::Result<(VideoCapture, LcdFrameCache, broadcast::Receiver<FrameInfo>)> {
        let camera_index: nokhwa::utils::CameraIndex =
            if let Ok(idx) = config.camera_index.parse::<u32>() {
                nokhwa::utils::CameraIndex::Index(idx)
            } else {
                nokhwa::utils::CameraIndex::String(config.camera_index.clone())
            };

        // 创建 broadcast 通道用于帧传递（带背压缓冲）
        let (frame_tx, frame_rx) = broadcast::channel::<FrameInfo>(100);
        let mut video_capture = VideoCapture::new(camera_index, frame_tx.clone(), config.rotation);
        video_capture.start_capture_frames_thread();

        let web_preview = WebPreview::new(8080, frame_tx, video_capture.resolution_arc());
        let lcd_frame_cache = Some(web_preview.lcd_frame_cache());

        // 启动 Web 服务器（使用正确的 frame_tx，确保能接收到视频帧）
        std::thread::spawn(move || {
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    log::error!("Failed to create tokio runtime: {}", e);
                    return;
                }
            };
            rt.block_on(web_preview.run());
        });

        Ok((video_capture, lcd_frame_cache, frame_rx))
    }

    /// 大语言模型线程, vosk返回的语音消息会丢入此线程解析, 当没联网时使用本地的qwen模型
    /// 如果是联网的模型只要实现`analyze_mood()`方法就好了
    ///
    /// # Arguments
    ///
    /// * `llm`:
    /// * `result_tx`:
    /// * `vm`:
    ///
    /// returns: ()
    ///
    /// # Examples
    ///
    /// ```
    ///
    /// ```
    fn llm_analysis_thread(
        llm: Arc<Mutex<QwenLlm>>,
        text_rx: mpsc::Receiver<String>,
        result_tx: Sender<Mood>,
        is_processing: Arc<AtomicBool>,
    ) {
        for text in text_rx {
            if !text.is_empty() {
                is_processing.store(true, std::sync::atomic::Ordering::Relaxed);
                let mood = llm
                    .lock()
                    .map_err(|e| {
                        log::error!("Failed to lock LLM: {}", e);
                        e
                    })
                    .ok()
                    .and_then(|mut guard| guard.analyze_mood(&text).ok())
                    .unwrap_or(Mood::Default);
                is_processing.store(false, std::sync::atomic::Ordering::Relaxed);
                let _ = result_tx.send(mood);
            }
        }
    }

    /// 连接机器人
    pub fn connect_robot(&mut self) {
        self.stop_comm_thread();
        self.popup.show_connecting();

        log::info!("Connecting to robot...");
        let (tx, rx) = mpsc::sync_channel(1);
        match robot::start_comm_thread(rx) {
            Ok((state, handle)) => {
                self.comm.state = Some(state);
                self.comm.thread = Some(handle);
                self.comm.tx = Some(tx);
                log::info!("Successfully connected to robot...");
            }
            Err(e) => {
                log::warn!("Failed to start comm thread: {e:?}");
            }
        }
        self.popup.hide();
    }

    /// 断开机器人连接
    pub fn stop_comm_thread(&mut self) {
        if let Some(tx) = self.comm.tx.take() {
            drop(tx);
        }
        if let Some(state) = &self.comm.state {
            robot::stop_comm_thread(state);
        }
        if let Some(handle) = self.comm.thread.take() {
            let _ = handle.join();
        }
        self.comm.state = None;
        self.popup.hide();
    }

    /// 发送帧数据 (原始像素数据)
    pub fn send_frame(&mut self) -> anyhow::Result<()> {
        let pixels = self.lcd.frame_vec();
        // 发送 LCD 帧到 Web 预览服务器
        if let Some(ref cache) = self.video.lcd_frame_cache {
            if let Ok(mut guard) = cache.lock() {
                *guard = Some(pixels.clone());
            }
        }

        // 如果开启了人脸追踪，获取人脸位置并通过 set_angle 直接修改共享状态
        if self.face_tracking_enabled {
            if let Some(face_x) = self.get_face_x_position() {
                log::info!("face_y :{:?}", face_x);
                let target_body = calculate_body_adjustment(face_x);
                let smoothed = smooth_adjustment(self.last_face_adjustment, target_body, 0.3);
                self.last_face_adjustment = smoothed;
                let current = self.joint.config().angles[5]; // 只改身体部分的数据
                let new_angle = (current + smoothed as f32).clamp(-90.0, 90.0);
                self.joint.set_angle(5, new_angle);
            }
        }

        // 发送时从共享状态读取最新的数据
        if let Some(tx) = &self.comm.tx {
            tx.try_send((pixels, self.joint.config()))?;
        }
        Ok(())
    }

    /// 获取当前人脸 x 坐标
    fn get_face_x_position(&mut self) -> Option<f32> {
        if let Some(rx) = &mut self.video.frame_rx {
            // 尝试接收最新帧，忽略过期帧
            while let Ok(frame_info) = rx.try_recv() {
                if frame_info.face_info.has_face {
                    return Some(frame_info.face_info.x);
                }
            }
        }
        None
    }

    /// 切换人脸追踪状态
    pub fn toggle_face_tracking(&mut self) {
        self.face_tracking_enabled = !self.face_tracking_enabled;
        if !self.face_tracking_enabled {
            // 关闭追踪时重置调整值
            self.last_face_adjustment = 0;
        }
        log::info!("Face tracking enabled: {}", self.face_tracking_enabled);
    }

    /// 截图并保存为 BMP 文件
    pub fn take_screenshot(&mut self) -> anyhow::Result<()> {
        let pixels = self.lcd.frame_vec();
        let img = image::RgbImage::from_raw(FRAME_WIDTH as u32, FRAME_HEIGHT as u32, pixels)
            .ok_or_else(|| anyhow::anyhow!("Invalid image dimensions"))?;
        // 生成文件名: screenshot_YYYYMMDD_HHMMSS.bmp
        let now = chrono::Local::now();
        let filename = format!(
            "./assets/images/screenshot/screenshot_{}.bmp",
            now.format("%Y%m%d_%H%M%S")
        );
        img.save(&filename)?;
        log::info!("Screenshot saved to: {filename}");

        Ok(())
    }

    pub fn quit(&mut self) {
        self.ui.running = false;
    }

    /// 使用 LLM 实例（懒加载）
    ///
    /// 如果 LLM 未初始化，会先尝试初始化。
    pub fn next_menu(&mut self) {
        self.select_menu(1);
    }

    pub fn prev_menu(&mut self) {
        self.select_menu(-1);
    }

    fn select_menu(&mut self, delta: isize) {
        let items = MenuItem::all();
        let len = items.len();
        let current = self.ui.menu_state.selected().unwrap_or(0);
        let new_i = (current as isize + delta).rem_euclid(len as isize) as usize;
        self.ui.menu_state.select(Some(new_i));
        self.ui.selected_menu = items[new_i];
    }

    /// 切换左右窗口焦点
    pub fn toggle_focus(&mut self) {
        self.ui.left_focused = !self.ui.left_focused;
    }

    /// 设置项数量
    pub fn settings_item_count(&self) -> usize {
        3 // Wifi名称, Wifi密码, 麦克风名称
    }

    pub fn settings_prev(&mut self) {
        self.move_settings(-1);
    }

    pub fn settings_next(&mut self) {
        self.move_settings(1);
    }

    fn move_settings(&mut self, delta: isize) {
        let count = self.settings_item_count();
        self.ui.settings_selected =
            (self.ui.settings_selected as isize + delta).rem_euclid(count as isize) as usize;
    }

    /// 保存设置项编辑内容
    pub fn save_settings_edit(&mut self) {
        match self.ui.settings_selected {
            0 => self.config.wifi_ssid = self.ui.edit_buffer.clone(),
            1 => self.config.wifi_password = self.ui.edit_buffer.clone(),
            2 => self.config.speech_name = self.ui.edit_buffer.clone(),
            _ => {}
        }
        if let Err(e) = self.config.save() {
            log::error!("Failed to save settings: {e}");
        }
        self.ui.in_edit_settings_mode = false;
        self.ui.edit_buffer.clear();
    }

    /// 取消设置项编辑
    pub fn cancel_settings_edit(&mut self) {
        self.ui.in_edit_settings_mode = false;
        self.ui.edit_buffer.clear();
    }

    pub fn is_connected(&self) -> bool {
        self.comm.state.is_some()
    }

    pub fn load_image_from_file(&mut self, path: &str) -> anyhow::Result<()> {
        self.lcd.load_image(path)?;
        self.lcd.set_mode(DisplayMode::Static);
        Ok(())
    }

    /// 根据 Mood 播放对应的 bibi 声
    fn play_beep_for_mood(mood: Mood) {
        match mood {
            Mood::Happy | Mood::Surprise => play_beep(2, 800.0, 100, 150),
            Mood::Angry | Mood::Sad | Mood::Confuse => play_beep(3, 500.0, 80, 100),
            Mood::Default | Mood::Loading => play_beep(1, 440.0, 150, 0),
        }
    }

    pub fn poll_voice_input(&mut self) {
        if let Some(rx) = &self.ai.voice_result_rx {
            while let Ok(mood) = rx.try_recv() {
                log::info!("Mood: {mood:?}");
                // 更新 LLM 测试状态
                if self.ui.in_llm_test_mode {
                    self.ai.llm_test_state.current_mood = Some(mood);
                    self.ai.llm_test_state.output_text = format!("情感: {:?}", mood);
                }
                self.ai
                    .is_processing
                    .store(false, std::sync::atomic::Ordering::Relaxed);
                self.lcd.set_eyes_mood(mood);
                Self::play_beep_for_mood(mood);
            }
        }
        if self
            .ai
            .is_processing
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            self.lcd.set_eyes_mood(Mood::Loading);
        }
    }
}

/// 通用弹窗配置
#[derive(Debug, Clone)]
pub struct PopupConfig {
    pub title: String,
    pub content: String,
    pub width: u16,
    pub height: u16,
    pub border_color: ratatui::style::Color,
    pub bg_color: ratatui::style::Color,
    pub title_color: ratatui::style::Color,
}

impl Default for PopupConfig {
    fn default() -> Self {
        Self {
            title: "弹窗".to_string(),
            content: "".to_string(),
            width: 40,
            height: 5,
            border_color: ratatui::style::Color::Green,
            bg_color: ratatui::style::Color::DarkGray,
            title_color: ratatui::style::Color::Cyan,
        }
    }
}

/// 通用弹窗
#[derive(Debug, Default)]
pub struct Popup {
    pub visible: bool,
    pub config: PopupConfig,
}

impl Popup {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn show(&mut self) {
        self.visible = true;
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn show_connecting(&mut self) {
        self.config = PopupConfig {
            title: " 连接设备 ".to_string(),
            content: "正在通过 USB 连接设备...".to_string(),
            ..PopupConfig::default()
        };
        self.show();
    }
}
