//! 服务端状态
//!
//! 集中持有所有硬件资源句柄, 以及向所有客户端广播事件的通道。
//! ws.rs 中的 WebSocket 处理器从此处读取/写入。

use crate::face_tracker::{calculate_body_adjustment, smooth_adjustment, BODY_SERVO_INDEX};
use crate::llm::{LlmManager, LlmResponse};
use crate::media::video::types::{FrameCache, FrameInfo};
use crate::media::video::VideoCapture;
use crate::media::voice::VoiceManager;
use crate::model_manager::ModelManager;
use crate::robot::{CommState, Joint, JointConfig, Lcd};
use boteyes::Mood;
use ele_bot_proto::{
    AppConfig, FacePosition, JointState, LlmResponse as ProtoLlmResponse, Mood as ProtoMood,
    ServerEvent,
};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::{broadcast, mpsc};

/// 发送给 USB 通信线程的通道
type BotFrameTx = std::sync::mpsc::SyncSender<(Vec<u8>, JointConfig)>;

/// 共享状态 - 通过 Arc 包裹后传给 WebSocket handler 和后台线程
pub struct SharedState {
    /// 应用配置
    pub config: RwLock<AppConfig>,
    /// 关节控制器
    pub joint: Arc<Joint>,
    /// LCD 帧渲染
    pub lcd: Mutex<Lcd>,
    /// 摄像头捕获
    pub video: Mutex<VideoCapture>,
    /// 语音/ASR/TTS - 用 Arc 包装以便在 WS 任务中安全共享
    pub voice: Mutex<Option<Arc<VoiceManager>>>,
    /// LLM 管理
    pub llm: Mutex<LlmManager>,
    /// 摄像头帧广播 (供 ws/web preview 订阅)
    pub frame_tx: FrameCache,
    /// 广播事件给所有 WS 客户端
    pub event_tx: broadcast::Sender<ServerEvent>,
    /// 发送给 USB 通信线程的通道
    pub bot_tx: Mutex<Option<BotFrameTx>>,
    /// 机器人通信状态(用于停止通信线程)
    pub comm_state: Mutex<Option<CommState>>,
    /// 机器人连接状态
    pub robot_connected: AtomicBool,
    /// LLM 处理中标志
    pub llm_processing: AtomicBool,
    /// LCD 帧缓存(Web 预览用)
    pub lcd_frame_cache: Arc<Mutex<Option<Vec<u8>>>>,
    /// 摄像头分辨率
    pub camera_resolution: Arc<Mutex<(u32, u32)>>,
    /// 文本输入通道(给 LLM 处理线程)
    pub llm_text_tx: Mutex<Option<mpsc::UnboundedSender<String>>>,
    /// 人脸追踪是否启用
    pub face_tracking_enabled: AtomicBool,
    /// 人脸追踪平滑状态(累计调整值, 度)
    face_tracking_adjustment: AtomicI32,
}

impl SharedState {
    /// 初始化所有硬件和后台资源
    pub fn new() -> anyhow::Result<Arc<Self>> {
        let config = AppConfig::load_or_default();

        // LCD
        let lcd = Lcd::new();

        // 关节
        let joint = Arc::new(Joint::new());

        // 摄像头
        let camera_index: nokhwa::utils::CameraIndex =
            if let Ok(idx) = config.camera_index.parse::<u32>() {
                nokhwa::utils::CameraIndex::Index(idx)
            } else {
                nokhwa::utils::CameraIndex::String(config.camera_index.clone())
            };

        let (frame_tx, _frame_rx) = broadcast::channel::<FrameInfo>(100);
        let mut video_capture = VideoCapture::new(
            camera_index,
            frame_tx.clone(),
            rotate_proto_to_local(config.rotation),
        );
        video_capture.start_capture_frames_thread();

        // 摄像头分辨率缓存
        let camera_resolution = video_capture.resolution_arc();

        // LCD 帧缓存
        let lcd_frame_cache = Arc::new(Mutex::new(None));

        // 语音
        log::debug!("start init voice");
        let voice = match Self::init_voice(&config) {
            Ok(m) => Some(Arc::new(m)),
            Err(e) => {
                log::warn!("init voice manager failed: {e}");
                None
            }
        };

        // LLM
        log::debug!("start init llm");
        let llm = Self::init_llm(&config)?;

        // 事件广播
        let (event_tx, _) = broadcast::channel::<ServerEvent>(1024);

        let state = Arc::new(Self {
            config: RwLock::new(config),
            joint,
            lcd: Mutex::new(lcd),
            video: Mutex::new(video_capture),
            voice: Mutex::new(voice),
            llm: Mutex::new(llm),
            frame_tx: frame_tx.clone(),
            event_tx,
            bot_tx: Mutex::new(None),
            comm_state: Mutex::new(None),
            robot_connected: AtomicBool::new(false),
            llm_processing: AtomicBool::new(false),
            lcd_frame_cache,
            camera_resolution,
            llm_text_tx: Mutex::new(None),
            face_tracking_enabled: AtomicBool::new(false),
            face_tracking_adjustment: AtomicI32::new(0),
        });

        // 启动 LLM 处理线程
        state.spawn_llm_thread();
        // 启动人脸追踪后台任务
        state.spawn_face_tracking_task(frame_tx);

        Ok(state)
    }

    fn init_llm(config: &AppConfig) -> anyhow::Result<LlmManager> {
        let mm = ModelManager::global();
        let Some(qw_tokenizer_path) = mm.get("tokenizer") else {
            anyhow::bail!("tokenizer not found");
        };
        let Some(qw_path) = mm.get("qwen") else {
            anyhow::bail!("qwen not found");
        };
        LlmManager::new(
            &config.llm_api_base,
            &config.llm_api_key,
            &config.llm_model,
            qw_path,
            qw_tokenizer_path,
        )
    }

    /// 用当前 `AppConfig` 重新构造 `VoiceManager`, 替换 `self.voice`.
    ///
    /// 在用户切换输入/输出设备后立即调用, 不需要重启服务. 旧实例
    /// 通过 `Arc` 引用计数归零自然 Drop: cpal Stream 停流, ASR 识别
    /// 线程的 `audio_rx` 收到断开信号后退出, 整个流程无需 join.
    ///
    /// # 软替换语义
    ///
    /// 旧实例的 `running` 标志先置 false, 给旧 ASR 线程最多 50ms
    /// 的窗口主动退出 (`asr::recognition_loop` 用 `recv_timeout(50ms)`
    /// 唤醒检查). 之后 sleep 60ms 确保旧线程已让出 cpal 设备, 再
    /// 构造新实例替换. 这样保证:
    ///
    /// 1. 旧 cpal Stream 不会继续向 mpsc 写音频
    /// 2. 旧 ASR 线程不会继续占用 sherpa-onnx 解码
    /// 3. 系统中同时最多只有一个 ASR 实例在跑
    ///
    /// # 为什么 sleep 60ms
    ///
    /// `asr::recognition_thread` 在 `audio_rx.recv_timeout(50ms)` 循环
    /// 里等待音频块; 只有等这次等待超时并 wake, 才能在下一行检查
    /// `running` 标志并主动退出. 50ms + 10ms 余量 = 60ms 是经验值,
    /// 保证旧线程在这个窗口里完成退出, 不被新 cpal Stream 抢占同一
    /// 设备的独占锁. **不要** 把这个 sleep 去掉或改短 — 会导致 Windows
    /// WASAPI 上 `Failed to bind audio device` 间歇性失败.
    ///
    /// # 为什么 drop old Arc 而不是 abort 旧 ASR 线程
    ///
    /// 旧 ASR 线程已经在 `running=false` 后主动退出, 没有强杀的必要.
    /// `Arc::drop` 让旧 cpal Stream 自然停流 (Drop 调 pause), 旧 sherpa-onnx
    /// 解码器随 `VoiceManager` 析构释放. 强行 abort 可能让 sherpa-onnx
    /// 内部状态损坏 (已经加载的模型可能 lock 住无法重建).
    ///
    /// # TTS 路径为什么不在这里 cancel
    ///
    /// ASR 是"长跑线程" (一直跑直到 running=false), 所以需要 cancel 信号.
    /// TTS 路径 (`VoiceManager::speak` / `speak_streaming`) 是阻塞调用,
    /// 跑在 `tokio::task::spawn_blocking` 里, 没人持有它的情况下用户发新
    /// SetConfig 会触发本函数; 旧 TTS 调用仍在跑, 旧 `VoiceManager` 还
    /// 被那个 spawn_blocking 闭包持有, 不会被 drop — 旧 device 句柄要等
    /// TTS 自然结束才释放. 这是已知设计取舍 ("切设备立即打断 TTS" 留
    /// 给未来 change). 见 `docs/voice-hot-swap.md`.
    ///
    /// # 失败语义
    ///
    /// 当 `init_voice` 返回 Err (例如新设备被独占占用) 时, 旧
    /// `VoiceManager` 已经因 take() 被移出但被外层 Arc 持有, 在函数
    /// 末尾 Drop — 旧 Stream 自然释放. 错误向上抛, 调用方 (ws.rs)
    /// 负责把错误通过 `ServerEvent::Error` 广播给客户端.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// if let Err(e) = state.rebuild_voice() {
    ///     let _ = state.event_tx.send(ServerEvent::Error { message: e.to_string() });
    /// }
    /// ```
    pub fn rebuild_voice(&self) -> anyhow::Result<()> {
        let config = self.config.read().unwrap().clone();

        // 1. 先把旧实例从 self.voice 移出 (但旧 Arc 还在本函数栈上,
        //    所以旧 cpal Stream 在这里还活着, 旧 ASR 线程继续跑).
        let old = {
            let mut guard = self.voice.lock().unwrap();
            guard.take()
        };
        if let Some(old) = &old {
            // 2. 通知旧 ASR 线程退出
            old.running()
                .store(false, std::sync::atomic::Ordering::Relaxed);
            // 3. 给退出窗口 (recv_timeout=50ms, 留 10ms 余量)
            std::thread::sleep(std::time::Duration::from_millis(60));
        }

        // 4. 构造新实例并替换 (旧 Arc 在本函数末尾 Drop)
        let new_voice = Self::init_voice(&config)?;
        *self.voice.lock().unwrap() = Some(Arc::new(new_voice));
        // 旧 Arc 在这里随 `old` 一起 Drop, 旧 cpal Stream 停流
        drop(old);
        Ok(())
    }

    /// 返回当前生效的 `(speech_name, output_device)`, 给 `set_config`
    /// 路径做"是否需要重建 VoiceManager"的判断.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let (old_mic, old_spk) = state.current_audio_config();
    /// if new_cfg.speech_name != old_mic || new_cfg.output_device != old_spk {
    ///     state.rebuild_voice()?;
    /// }
    /// ```
    pub fn current_audio_config(&self) -> (String, String) {
        let cfg = self.config.read().unwrap();
        (cfg.speech_name.clone(), cfg.output_device.clone())
    }

    fn init_voice(config: &AppConfig) -> anyhow::Result<VoiceManager> {
        use crate::media::voice::{AsrModelPaths, TtsModelPaths};

        let mm = ModelManager::global();
        if let (
            Some(sense_voice_path),
            Some(silero_vad_path),
            Some(tokens_path),
            Some(tts_model_path),
            Some(tts_tokens_path),
            Some(tts_lexicon_path),
        ) = (
            mm.get("sense_voice"),
            mm.get("silero_vad"),
            mm.get("sense_voice_tokens"),
            mm.get("vits_tts"),
            mm.get("vits_tts_tokens"),
            mm.get("vits_tts_lexicon"),
        ) {
            VoiceManager::new(
                AsrModelPaths::new(sense_voice_path, silero_vad_path, tokens_path),
                TtsModelPaths::new(tts_model_path, tts_tokens_path, tts_lexicon_path),
                &config.speech_name,
                config.speech_device_id.as_deref(),
                &config.output_device,
                config.output_device_id.as_deref(),
            )
        } else {
            anyhow::bail!("voice model not available");
        }
    }

    /// 启动 LLM 处理线程(消费 mpsc 文本, 调用 LLM 分析, 广播结果)
    fn spawn_llm_thread(self: &Arc<Self>) {
        let (text_tx, mut text_rx) = mpsc::unbounded_channel::<String>();
        *self.llm_text_tx.lock().unwrap() = Some(text_tx);

        let state = self.clone();
        std::thread::spawn(move || {
            while let Some(text) = text_rx.blocking_recv() {
                if text.is_empty() {
                    continue;
                }
                state.llm_processing.store(true, Ordering::Relaxed);
                let _ = state.event_tx.send(ServerEvent::LlmProcessing {
                    is_processing: true,
                });

                let response = {
                    let llm = state.llm.lock().unwrap();
                    llm.analyze_mood(&text).unwrap_or_else(|e| {
                        log::warn!("analyze_mood failed: {e:?}");
                        LlmResponse::default()
                    })
                };

                state.llm_processing.store(false, Ordering::Relaxed);
                let _ = state.event_tx.send(ServerEvent::LlmProcessing {
                    is_processing: false,
                });

                let proto_response = ProtoLlmResponse {
                    mood: mood_to_proto(response.mood),
                    actions: response.actions.iter().map(action_to_proto).collect(),
                };
                let _ = state.event_tx.send(ServerEvent::LlmResponse {
                    response: proto_response,
                });

                if let Ok(mut lcd) = state.lcd.lock() {
                    lcd.set_eyes_mood(response.mood);
                }
            }
        });
    }

    /// 启动人脸追踪后台任务
    ///
    /// 订阅 `frame_tx` 广播, 收到带人脸检测结果的帧时:
    /// - 若 `face_tracking_enabled`, 计算身体舵机调整量并直接 set_angle
    /// - 始终广播 `ServerEvent::Face` 给所有 WS 客户端(用于 UI 显示)
    fn spawn_face_tracking_task(self: &Arc<Self>, frame_tx: FrameCache) {
        let mut frame_rx = frame_tx.subscribe();
        let state = self.clone();
        std::thread::spawn(move || {
            loop {
                match frame_rx.blocking_recv() {
                    Ok(frame_info) => {
                        let position = FacePosition {
                            x: frame_info.face_info.x,
                            has_face: frame_info.face_info.has_face,
                        };

                        // 始终广播给客户端(可选 UI 显示)
                        let _ = state.event_tx.send(ServerEvent::Face { position });

                        // 仅在追踪开启时调整舵机
                        if state.face_tracking_enabled.load(Ordering::Relaxed) && position.has_face
                        {
                            let target = calculate_body_adjustment(position.x);
                            let prev = state.face_tracking_adjustment.load(Ordering::Relaxed);
                            let smoothed = smooth_adjustment(prev, target, 0.3);
                            state
                                .face_tracking_adjustment
                                .store(smoothed, Ordering::Relaxed);

                            let current_angle = state.joint.values()[BODY_SERVO_INDEX];
                            let new_angle =
                                (current_angle as f32 + smoothed as f32).clamp(-90.0, 90.0);
                            state.joint.set_angle(BODY_SERVO_INDEX, new_angle);
                        } else if !position.has_face
                            && state.face_tracking_enabled.load(Ordering::Relaxed)
                        {
                            // 无人脸时, 平滑回 0
                            let prev = state.face_tracking_adjustment.load(Ordering::Relaxed);
                            let smoothed = smooth_adjustment(prev, 0, 0.1);
                            state
                                .face_tracking_adjustment
                                .store(smoothed, Ordering::Relaxed);
                            if smoothed == 0 && prev != 0 {
                                // 已归零, 复位身体舵机
                                state.joint.set_angle(BODY_SERVO_INDEX, 0.0);
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        log::debug!("face tracking lagged, dropped {n} frames");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    /// 启用/禁用人脸追踪; 禁用时复位累计调整值
    pub fn set_face_tracking(&self, enabled: bool) {
        self.face_tracking_enabled.store(enabled, Ordering::Relaxed);
        if !enabled {
            self.face_tracking_adjustment.store(0, Ordering::Relaxed);
        }
    }

    /// 生成当前 LCD 帧数据
    pub fn generate_lcd_frame(&self) -> Vec<u8> {
        if let Ok(mut lcd) = self.lcd.lock() {
            lcd.frame_vec()
        } else {
            Vec::new()
        }
    }

    /// 发送 LCD 帧到 USB 通信线程
    pub fn push_frame_to_robot(&self, pixels: Vec<u8>) {
        if let Some(tx) = self.bot_tx.lock().unwrap().as_ref() {
            let joint_config = self.joint.config();
            let _ = tx.try_send((pixels, joint_config));
        }
    }

    /// 切换眼睛情绪
    pub fn set_mood(&self, mood: Mood) {
        if let Ok(mut lcd) = self.lcd.lock() {
            lcd.set_eyes_mood(mood);
        }
    }

    /// 通知连接状态变化
    pub fn notify_connection(&self, is_connected: bool) {
        self.robot_connected.store(is_connected, Ordering::Relaxed);
        let _ = self.event_tx.send(ServerEvent::Connection { is_connected });
    }

    /// 停止机器人通信线程
    pub fn stop_robot_comm(&self) {
        *self.bot_tx.lock().unwrap() = None;
        if let Some(state) = self.comm_state.lock().unwrap().take() {
            crate::robot::stop_comm_thread(&state);
        }
    }

    /// 截图并保存
    pub fn take_screenshot(&self) -> anyhow::Result<String> {
        use electron_bot::{FRAME_HEIGHT, FRAME_WIDTH};
        let pixels = self.generate_lcd_frame();
        let img = image::RgbImage::from_raw(FRAME_WIDTH as u32, FRAME_HEIGHT as u32, pixels)
            .ok_or_else(|| anyhow::anyhow!("Invalid image dimensions"))?;
        let now = chrono::Local::now();
        let filename = format!(
            "./assets/images/screenshot/screenshot_{}.bmp",
            now.format("%Y%m%d_%H%M%S")
        );
        img.save(&filename)?;
        Ok(filename)
    }

    /// 广播舵机状态
    pub fn broadcast_joint_state(&self) {
        let state = JointState {
            values: self.joint.values(),
            selected: self.joint.selected(),
        };
        let _ = self.event_tx.send(ServerEvent::JointState { state });
    }

    /// 广播当前 JointConfig(用于预览/调试)
    pub fn broadcast_joint_config(&self) {
        let _ = self.event_tx.send(ServerEvent::JointConfig {
            config: joint_config_to_proto(&self.joint.config()),
        });
    }

    /// 获取当前 config
    pub fn config(&self) -> AppConfig {
        self.config.read().unwrap().clone()
    }

    /// 更新 config
    ///
    /// 若 `speech_name` / `output_device` 与旧值不同, 立即重建
    /// `VoiceManager` (热生效). 重建失败时 config 仍按本次值持久化
    /// (用户已经表达过选择意图), 但通过 `ServerEvent::Error` 告知
    /// 客户端当前 ASR/TTS 仍在用旧设备.
    pub fn set_config(&self, cfg: AppConfig) -> anyhow::Result<()> {
        let (old_mic, old_spk) = self.current_audio_config();
        let audio_changed = cfg.speech_name != old_mic || cfg.output_device != old_spk;
        cfg.save()?;
        *self.config.write().unwrap() = cfg.clone();
        let _ = self.event_tx.send(ServerEvent::Config { config: cfg });
        if audio_changed {
            if let Err(e) = self.rebuild_voice() {
                log::warn!("rebuild_voice failed: {e:?}");
                let _ = self.event_tx.send(ServerEvent::Error {
                    message: format!("voice rebuild failed: {e}"),
                });
            }
        }
        Ok(())
    }
}

/// proto::Mood -> boteyes::Mood
pub fn mood_from_proto(m: ProtoMood) -> Mood {
    match m {
        ProtoMood::Default => Mood::Default,
        ProtoMood::Happy => Mood::Happy,
        ProtoMood::Sad => Mood::Sad,
        ProtoMood::Angry => Mood::Angry,
        ProtoMood::Surprise => Mood::Surprise,
        ProtoMood::Confuse => Mood::Confuse,
        ProtoMood::Loading => Mood::Loading,
    }
}

/// boteyes::Mood -> proto::Mood
pub fn mood_to_proto(m: Mood) -> ProtoMood {
    match m {
        Mood::Default => ProtoMood::Default,
        Mood::Happy => ProtoMood::Happy,
        Mood::Sad => ProtoMood::Sad,
        Mood::Angry => ProtoMood::Angry,
        Mood::Surprise => ProtoMood::Surprise,
        Mood::Confuse => ProtoMood::Confuse,
        Mood::Loading => ProtoMood::Loading,
    }
}

/// proto::RotateAngle -> 内部 video::process::RotateAngle
pub fn rotate_proto_to_local(
    r: ele_bot_proto::RotateAngle,
) -> crate::media::video::process::RotateAngle {
    use crate::media::video::process::RotateAngle as Local;
    match r {
        ele_bot_proto::RotateAngle::Rotate0 => Local::None,
        ele_bot_proto::RotateAngle::Rotate90 => Local::Rotate90,
        ele_bot_proto::RotateAngle::Rotate180 => Local::Rotate180,
        ele_bot_proto::RotateAngle::Rotate270 => Local::Rotate270,
    }
}

/// 内部 Action -> proto::Action
pub fn action_to_proto(a: &crate::llm::response::Action) -> ele_bot_proto::Action {
    ele_bot_proto::Action {
        servo_index: a.servo_index,
        angle: a.angle,
        duration_ms: a.duration_ms,
    }
}

/// 内部 JointConfig -> proto::JointConfig
pub fn joint_config_to_proto(c: &crate::robot::JointConfig) -> ele_bot_proto::JointConfig {
    ele_bot_proto::JointConfig {
        enable: c.enable,
        angles: c.angles,
    }
}
