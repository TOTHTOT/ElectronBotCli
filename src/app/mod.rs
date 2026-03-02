pub mod config;
/// app模块, 负责界面调度以及实际运行功能
pub mod menu;

use crate::llm::QwenLlm;
use crate::robot::{self, CommState, DisplayMode, Joint, JointConfig, Lcd};
use crate::ui::pages::llm_test::LlmTestState;
use crate::web::WebPreview;
use std::default::Default;
use std::sync::{Arc, Mutex};

// 导出菜单
pub use menu::*;

use crate::voice::play_beep;
use crate::voice::VoiceManager;
use boteyes::Mood;
use electron_bot::{FRAME_HEIGHT, FRAME_WIDTH};
use ratatui::widgets::ListState;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Sender, SyncSender};

pub type BotRecvType = (Vec<u8>, JointConfig);

/// 主应用
pub struct App {
    pub menu_state: ListState,
    pub selected_menu: MenuItem,
    pub running: bool,
    pub joint: Joint,
    pub in_servo_mode: bool,
    pub in_settings: bool,
    pub settings_selected: usize,
    pub in_edit_settings_mode: bool,
    pub edit_buffer: String,
    pub config: config::AppConfig,
    pub lcd: Lcd,
    pub popup: Popup,
    pub voice_manager: Option<Arc<VoiceManager>>,
    voice_result_rx: Option<mpsc::Receiver<Mood>>,
    pub left_focused: bool,
    pub is_processing: Arc<AtomicBool>,
    pub in_llm_test_mode: bool,
    pub llm_test_state: LlmTestState,
    pub text_tx: Sender<String>,
    #[allow(dead_code)]
    llm: Option<Arc<Mutex<QwenLlm>>>,
    comm_state: Option<CommState>,
    comm_thread: Option<std::thread::JoinHandle<()>>,
    comm_tx: Option<SyncSender<BotRecvType>>,
    /// Web 预览服务器
    web_preview: Option<Arc<WebPreview>>,
    /// LCD 帧缓存（用于 Web 预览）
    lcd_frame_cache: Option<std::sync::Arc<std::sync::Mutex<Option<Vec<u8>>>>>,
}

#[allow(dead_code)]
impl App {
    pub fn new(llm: QwenLlm) -> Self {
        let mut menu_state = ListState::default();
        menu_state.select(Some(0));

        let lcd = Lcd::new();
        let config = config::AppConfig::load();

        let llm_arc = Arc::new(Mutex::new(llm));
        let is_processing = Arc::new(AtomicBool::new(false));
        let is_processing_clone = is_processing.clone();
        let (result_tx, result_rx) = mpsc::channel();

        let (text_tx, text_rx) = mpsc::channel::<String>();
        let text_tx_clone = text_tx.clone();

        // 启动 LLM 处理线程
        let llm_for_thread = llm_arc.clone();
        let result_tx_for_llm = result_tx.clone();
        std::thread::spawn(move || {
            Self::llm_analysis_thread(
                llm_for_thread,
                text_rx,
                result_tx_for_llm,
                is_processing_clone,
            );
        });

        // 启动语音识别
        let voice_manager = VoiceManager::new(
            "external/module/sense_voice/model.int8.onnx".into(),
            "external/module/silero_vad/silero_vad.onnx".into(),
            "external/module/vits/tokens.txt",
            &config.speech_name,
            text_tx_clone,
        )
        .map(Arc::new)
        .ok();

        // 创建 Web 预览服务器
        let web_port = 8080;
        let web_preview = WebPreview::new(web_port);
        let lcd_frame_cache = Some(web_preview.lcd_frame_cache());

        // 启动 Web 服务器（异步运行）
        let web_preview_arc = Arc::new(web_preview.clone());
        let web_preview_for_thread = (*web_preview_arc).clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(web_preview_for_thread.run());
        });

        log::info!("Web preview server started at http://localhost:{}", web_port);

        Self {
            menu_state,
            selected_menu: MenuItem::DeviceStatus,
            running: true,
            joint: Joint::new(),
            in_servo_mode: false,
            in_settings: false,
            settings_selected: 0,
            in_edit_settings_mode: false,
            edit_buffer: String::new(),
            config,
            lcd,
            popup: Popup::new(),
            voice_manager,
            voice_result_rx: Some(result_rx),
            left_focused: true,
            is_processing,
            in_llm_test_mode: false,
            llm_test_state: LlmTestState::default(),
            text_tx,
            llm: Some(llm_arc),
            comm_state: None,
            comm_thread: None,
            comm_tx: None,
            web_preview: Some(web_preview_arc),
            lcd_frame_cache,
        }
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
                    .unwrap()
                    .analyze_mood(&text)
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
                self.comm_state = Some(state);
                self.comm_thread = Some(handle);
                self.comm_tx = Some(tx);
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
        if let Some(tx) = self.comm_tx.take() {
            drop(tx);
        }
        if let Some(state) = &self.comm_state {
            robot::stop_comm_thread(state);
        }
        if let Some(handle) = self.comm_thread.take() {
            let _ = handle.join();
        }
        self.comm_state = None;
        self.popup.hide();
    }

    /// 发送帧数据 (原始像素数据)
    pub fn send_frame(&mut self) -> anyhow::Result<()> {
        if let Some(tx) = &self.comm_tx {
            let pixels = self.lcd.frame_vec();
            // 发送 LCD 帧到 Web 预览服务器
            if let Some(ref cache) = self.lcd_frame_cache {
                if let Ok(mut guard) = cache.lock() {
                    *guard = Some(pixels.clone());
                }
            }
            let config = self.joint.config();
            tx.try_send((pixels, config))?;
        }
        Ok(())
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
        self.running = false;
    }

    pub fn next_menu(&mut self) {
        let items = MenuItem::all();
        let i = match self.menu_state.selected() {
            Some(i) => (i + 1) % items.len(),
            None => 0,
        };
        self.menu_state.select(Some(i));
        self.selected_menu = items[i];
    }

    pub fn prev_menu(&mut self) {
        let items = MenuItem::all();
        let i = match self.menu_state.selected() {
            Some(i) => (i + items.len() - 1) % items.len(),
            None => 0,
        };
        self.menu_state.select(Some(i));
        self.selected_menu = items[i];
    }

    /// 切换左右窗口焦点
    pub fn toggle_focus(&mut self) {
        self.left_focused = !self.left_focused;
    }

    /// 设置项数量
    pub fn settings_item_count(&self) -> usize {
        3 // Wifi名称, Wifi密码, 麦克风名称
    }

    /// 设置模式: 上一项
    pub fn settings_prev(&mut self) {
        let count = self.settings_item_count();
        self.settings_selected = (self.settings_selected + count - 1) % count;
    }

    /// 设置模式: 下一项
    pub fn settings_next(&mut self) {
        let count = self.settings_item_count();
        self.settings_selected = (self.settings_selected + 1) % count;
    }

    /// 保存设置项编辑内容
    pub fn save_settings_edit(&mut self) {
        match self.settings_selected {
            0 => self.config.wifi_ssid = self.edit_buffer.clone(),
            1 => self.config.wifi_password = self.edit_buffer.clone(),
            2 => self.config.speech_name = self.edit_buffer.clone(),
            _ => {}
        }
        if let Err(e) = self.config.save() {
            log::error!("Failed to save settings: {e}");
        }
        self.in_edit_settings_mode = false;
        self.edit_buffer.clear();
    }

    /// 取消设置项编辑
    pub fn cancel_settings_edit(&mut self) {
        self.in_edit_settings_mode = false;
        self.edit_buffer.clear();
    }

    pub fn is_connected(&self) -> bool {
        self.comm_state.is_some()
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
        if let Some(rx) = &self.voice_result_rx {
            while let Ok(mood) = rx.try_recv() {
                log::info!("Mood: {mood:?}");
                // 更新 LLM 测试状态
                if self.in_llm_test_mode {
                    self.llm_test_state.current_mood = Some(mood);
                    self.llm_test_state.output_text = format!("情感: {:?}", mood);
                }
                self.is_processing
                    .store(false, std::sync::atomic::Ordering::Relaxed);
                self.lcd.set_eyes_mood(mood);
                Self::play_beep_for_mood(mood);
            }
        }
        if self
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
        Self {
            visible: false,
            config: PopupConfig::default(),
        }
    }

    /// 显示弹窗
    pub fn show(&mut self) {
        self.visible = true;
    }

    /// 隐藏弹窗
    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// 是否可见
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// 设置配置
    pub fn configure(&mut self, config: PopupConfig) {
        self.config = config;
    }

    /// 快速设置连接中弹窗
    pub fn show_connecting(&mut self) {
        self.configure(PopupConfig {
            title: " 连接设备 ".to_string(),
            content: "正在通过 USB 连接设备...".to_string(),
            width: 40,
            height: 5,
            border_color: ratatui::style::Color::Green,
            bg_color: ratatui::style::Color::DarkGray,
            title_color: ratatui::style::Color::Cyan,
        });
        self.show();
    }
}
