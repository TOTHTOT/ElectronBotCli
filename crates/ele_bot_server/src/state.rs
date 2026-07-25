//! 服务端状态
//!
//! 集中持有所有硬件资源句柄, 以及向所有客户端广播事件的通道。
//! ws.rs 中的 WebSocket 处理器从此处读取/写入。

use crate::event_bus::{BusEvent, EventBus};
use crate::face_tracker::{calculate_body_adjustment, smooth_adjustment, BODY_SERVO_INDEX};
use crate::llm::{LlmManager, LlmResponse};
use crate::media::video::capture::parse_camera_index;
use crate::media::video::VideoCapture;
use crate::media::voice::VoiceManager;
use crate::model_manager::ModelManager;
use crate::robot::{CommState, Joint, JointConfig, Lcd};
use crate::web::WebPreview;
use boteyes::Mood;
use ele_bot_proto::{
    AppConfig, FacePosition, JointState, LlmResponse as ProtoLlmResponse, Mood as ProtoMood,
    ServerEvent,
};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::broadcast;

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
    /// 摄像头捕获 - 用 `Mutex<Option<Arc>>` 包装以便热切换 (`rebuild_video`).
    /// `None` 表示视频子系统不可用 (启动时枚举失败或热重建中途的中间态).
    /// `Arc` 让旧实例能在取走后被并发任务继续持有, 等到引用计数归零才 Drop.
    pub video: Mutex<Option<Arc<VideoCapture>>>,
    /// 语音/ASR/TTS - 用 Arc 包装以便在 WS 任务中安全共享
    pub voice: Mutex<Option<Arc<VoiceManager>>>,
    /// LLM 管理
    pub llm: tokio::sync::Mutex<LlmManager>,
    /// 摄像头帧广播 (供 ws/web preview 订阅)
    // pub frame_tx: FrameCache,
    /// 事件总线 - 替代原 `event_tx` (broadcast `ServerEvent`) + `llm_text_tx` (mpsc String)
    /// + `voice.asr_text_rx` 三处手工 channel. 新订阅者调 `bus_tx.subscribe()` 即可.
    pub bus_tx: EventBus,
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
    /// 人脸追踪是否启用
    pub face_tracking_enabled: AtomicBool,
    /// 人脸追踪平滑状态(累计调整值, 度)
    face_tracking_adjustment: AtomicI32,
    // _web: WebPreview,
}

impl SharedState {
    /// 初始化所有硬件和后台资源
    pub async fn new() -> anyhow::Result<Arc<Self>> {
        let config = AppConfig::load_or_default();

        // 容量 1024 跟原 broadcast 一致.
        let bus_tx = EventBus::new(1024);

        // LCD
        let lcd = Lcd::new();

        // 关节
        let joint = Arc::new(Joint::new());

        // 摄像头
        let camera_index = parse_camera_index(&config.camera_index);

        let mut video_capture = VideoCapture::new(
            camera_index,
            bus_tx.clone(),
            rotate_proto_to_local(config.rotation),
        );
        let web = WebPreview::new(7777, bus_tx.subscribe(), video_capture.resolution_arc());
        tokio::spawn(async move {
            web.run().await;
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
        });
        video_capture.start_capture_frames_thread();

        // 摄像头分辨率缓存 - 直接持有 Arc<Mutex<(w, h)>> 让分辨率跨实例共享
        // (旧实例 Drop 不影响新实例写分辨率, 反之亦然)
        let camera_resolution = video_capture.resolution_arc();
        // LCD 帧缓存
        let lcd_frame_cache = Arc::new(Mutex::new(None));

        // 语音
        log::debug!("start init voice");
        let voice = match Self::init_voice(&config, bus_tx.clone()) {
            Ok(m) => Some(Arc::new(m)),
            Err(e) => {
                log::warn!("init voice manager failed: {e}");
                None
            }
        };

        // LLM
        log::debug!("start init llm");
        let llm = Self::init_llm(&config)?;

        let state = Arc::new(Self {
            config: RwLock::new(config),
            joint,
            lcd: Mutex::new(lcd),
            video: Mutex::new(Some(Arc::new(video_capture))),
            voice: Mutex::new(voice),
            llm: tokio::sync::Mutex::new(llm),
            // frame_tx: frame_tx.clone(),
            bus_tx: bus_tx.clone(),
            bot_tx: Mutex::new(None),
            comm_state: Mutex::new(None),
            robot_connected: AtomicBool::new(false),
            llm_processing: AtomicBool::new(false),
            lcd_frame_cache,
            camera_resolution,
            face_tracking_enabled: AtomicBool::new(false),
            face_tracking_adjustment: AtomicI32::new(0),
            // _web: web,
        });

        // 启动 LLM 处理 tokio task (订阅 EventBus::AsrText)
        state.spawn_llm_thread();
        // 启动 TTS trigger tokio task (订阅 EventBus::LlmReply)
        state.spawn_tts_trigger_thread();
        // 启动人脸追踪后台任务
        state.spawn_face_tracking_task().await;

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
    /// `SetConfig` 会触发本函数; 旧 TTS 调用仍在跑, 旧 `VoiceManager` 还
    /// 被那个 `spawn_blocking` 闭包持有, 不会被 drop — 旧 device 句柄要等
    /// TTS 自然结束才释放. 这是已知设计取舍 ("切设备立即打断 TTS" 留
    /// 给未来 change). 见 `docs/voice-hot-swap.md`.
    ///
    /// # 失败语义
    ///
    /// 当 `init_voice` 返回 Err (例如新设备被独占占用) 时, 旧
    /// `VoiceManager` 已经因 `take()` 被移出但被外层 Arc 持有, 在函数
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
            old.running().store(false, Ordering::Relaxed);
            // 3. 给退出窗口 (recv_timeout=50ms, 留 10ms 余量)
            std::thread::sleep(std::time::Duration::from_millis(60));
        }

        // 4. 构造新实例并替换 (旧 Arc 在本函数末尾 Drop)
        let new_voice = Self::init_voice(&config, self.bus_tx.clone())?;
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

    /// 返回当前生效的 `camera_index`, 给 `set_config` 路径做
    /// "是否需要重建 VideoCapture" 的判断. 与 `current_audio_config` 对称.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let old_idx = state.current_video_config();
    /// if new_cfg.camera_index != old_idx {
    ///     state.rebuild_video()?;
    /// }
    /// ```
    #[must_use]
    pub fn current_video_config(&self) -> String {
        self.config.read().unwrap().camera_index.clone()
    }

    /// 获取当前视频实例的 `Arc` 快照. 热切换后调用此方法拿到的就是
    /// 新实例; 旧实例的 `Arc` 还在别处被持有 (face tracking / web preview)
    /// 则仍可用, 不会因为 `take()` 而立即析构.
    #[must_use]
    pub fn video(&self) -> Option<Arc<VideoCapture>> {
        self.video.lock().unwrap().clone()
    }

    /// 用当前 `AppConfig` 重新构造 `VideoCapture`, 替换 `self.video`.
    ///
    /// 用户在 picker 切换摄像头后, `set_config` 检测到 `camera_index` 变化
    /// 立即调此函数, 不重启 ws 服务.
    ///
    /// # 替换语义 (与 [`Self::rebuild_voice`] 对齐)
    ///
    /// 1. `take()` 移出旧实例 (旧 `Arc` 还在本函数栈, capture frame 线程继续跑)
    /// 2. 函数末尾 Drop 旧 `Arc` → `VideoCapture::Drop` 自动 `running=false`
    ///    + `handle.join()` (capture thread 等当前一帧抓完再退出, 约几十毫秒)
    /// 3. 用新 `CameraIndex` 构造 `VideoCapture` + `start_capture_frames_thread`
    ///    → 推回 `Option`
    ///
    /// 与 audio 端不同: audio 端旧实例 Drop 让 cpal Stream 自然停流,
    /// 摄像头端 `VideoCapture::Drop` 会同步 join capture thread, 阻塞
    /// 当前函数 (调用方 `set_config`), 期间 ws 任务在 `handle_command`
    /// `await` 同步段. 这是已知设计取舍 — 摄像头比音频"卡顿"一点
    /// 但不影响用户体验.
    ///
    /// # 失败语义
    ///
    /// `VideoCapture::new` 报错时 (设备被独占 / 路径不存在), 函数返回
    /// `Err`, 调用方 (`set_config`) 负责推 `ServerEvent::Error` 给客户端.
    /// 此函数**不会**自动回退旧 index —— 设计上切成"用户已表达选择
    /// 意图", 失败由 `set_config` 用旧 index 重建一个 fallback 实例保证
    /// 视频流不断.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// if let Err(e) = state.rebuild_video() {
    ///     state.bus_tx.publish(BusEvent::ServerEvent(ServerEvent::Error {
    ///         message: format!("camera rebuild failed: {e}"),
    ///     }));
    /// }
    /// ```
    pub fn rebuild_video(&self) -> anyhow::Result<()> {
        let config = self.config.read().unwrap().clone();
        let new_index = parse_camera_index(&config.camera_index);
        let rotation = rotate_proto_to_local(config.rotation);

        // 1. 移出旧实例. 旧 Arc 仍在本函数栈, 所以 capture thread 还在跑.
        let old = {
            let mut guard = self.video.lock().unwrap();
            guard.take()
        };

        // 2. 构造新实例. 即使中途失败, "旧实例已 take + 新实例尚未替换"
        // 这个中间态会在 Err 返回时被调用方 (set_config fallback) 接手.
        let mut new_capture = VideoCapture::new(new_index, self.bus_tx.clone(), rotation);
        new_capture.start_capture_frames_thread();
        *self.video.lock().unwrap() = Some(Arc::new(new_capture));

        // 3. 替换成功后 Drop 旧实例 — Drop impl 会 join capture thread,
        // 等当前帧抓完自然退出.
        drop(old);
        Ok(())
    }

    fn init_voice(config: &AppConfig, bus: EventBus) -> anyhow::Result<VoiceManager> {
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
                bus,
            )
        } else {
            anyhow::bail!("voice model not available");
        }
    }

    /// 启动 LLM 处理任务 (tokio task, 订阅 `EventBus::AsrText`).
    ///
    /// 流程: `AsrText` → chat (生成回复) + `analyze_mood` (情感+动作) → 发布
    /// `ServerEvent::LlmResponse` + `BusEvent::LlmReply` (供 TTS trigger 消费).
    fn spawn_llm_thread(self: &Arc<Self>) {
        let state = self.clone();
        let mut rx = state.bus_tx.subscribe();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(BusEvent::AsrText(text)) => {
                        if text.is_empty() {
                            continue;
                        }
                        log::debug!("LLM task received: {text}");
                        state.llm_processing.store(true, Ordering::Relaxed);
                        state
                            .bus_tx
                            .publish(BusEvent::ServerEvent(ServerEvent::LlmProcessing {
                                is_processing: true,
                            }));

                        // 阶段 1: chat — 生成对话文本.
                        // state.llm 是 tokio::sync::Mutex, lock() 返 future.
                        // await 拿 Guard, Guard 跨 await 安全 (tokio Mutex 设计).
                        let reply_text =
                            state
                                .llm
                                .lock()
                                .await
                                .chat(&text)
                                .await
                                .unwrap_or_else(|e| {
                                    log::warn!("chat failed: {e:?}");
                                    format!("[LLM 错误: {e}]")
                                });
                        log::info!("LLM reply: {reply_text}");

                        // 阶段 2: analyze_mood — 情感 + 舵机动作. analyze_mood 也是 async.
                        let response = state
                            .llm
                            .lock()
                            .await
                            .analyze_mood(&text)
                            .await
                            .unwrap_or_else(|e| {
                                log::warn!("analyze_mood failed: {e:?}");
                                LlmResponse::default()
                            });

                        state.llm_processing.store(false, Ordering::Relaxed);
                        state
                            .bus_tx
                            .publish(BusEvent::ServerEvent(ServerEvent::LlmProcessing {
                                is_processing: false,
                            }));

                        let proto_response = ProtoLlmResponse {
                            mood: mood_to_proto(response.mood),
                            actions: response.actions.iter().map(action_to_proto).collect(),
                            reply_text: if reply_text.is_empty() {
                                None
                            } else {
                                Some(reply_text.clone())
                            },
                        };
                        state
                            .bus_tx
                            .publish(BusEvent::ServerEvent(ServerEvent::LlmResponse {
                                response: proto_response,
                            }));

                        if let Ok(mut lcd) = state.lcd.lock() {
                            lcd.set_eyes_mood(response.mood);
                        }

                        // 发布 LlmReply, 触发 TTS trigger 任务播报.
                        if !reply_text.is_empty() {
                            state.bus_tx.publish(BusEvent::LlmReply(reply_text));
                        }
                    }
                    Ok(_) => continue,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        log::warn!("LLM task lagged, dropped {n} events");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        log::warn!("LLM task closed");
                        break;
                    }
                }
            }
        });
    }

    /// 启动 TTS 触发任务 (tokio task, 订阅 `EventBus::LlmReply`).
    ///
    /// LLM 任务发布 `BusEvent::LlmReply` 后, 这里收到就调 `voice.speak`
    /// 触发 TTS 播报. 用 `spawn_blocking` 异步不阻塞 bus 消费循环.
    /// `VoiceManager` 不可用 (热重建中) 时 log warn 跳过.
    fn spawn_tts_trigger_thread(self: &Arc<Self>) {
        let state = self.clone();
        let mut rx = state.bus_tx.subscribe();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(BusEvent::LlmReply(text)) => {
                        if text.is_empty() {
                            continue;
                        }
                        log::info!("TTS trigger: {text}");
                        let voice_opt = state.voice.lock().unwrap().clone();
                        match voice_opt {
                            Some(voice) => {
                                let text_for_tts = text;
                                tokio::task::spawn_blocking(move || {
                                    if let Err(e) = voice.speak(&text_for_tts, 1.0, None) {
                                        log::warn!("TTS playback failed: {e:?}");
                                    }
                                });
                            }
                            None => {
                                log::warn!("voice manager not available for TTS");
                            }
                        }
                    }
                    Ok(_) => continue,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        log::warn!("TTS trigger lagged, dropped {n} events");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    /// 启动人脸追踪后台任务
    ///
    /// # Arguments
    ///
    ///
    /// returns: ()
    ///
    /// # Examples
    ///
    /// ```
    ///
    /// ```
    async fn spawn_face_tracking_task(self: &Arc<Self>) {
        let state = self.clone();
        tokio::spawn(async move {
            loop {
                match state.bus_tx.subscribe().recv().await {
                    Ok(frame_info) => {
                        if let BusEvent::CameraVideo(video) = frame_info {
                            let position = FacePosition {
                                x: video.face_info.x,
                                has_face: video.face_info.has_face,
                            };

                            // 始终广播给客户端(可选 UI 显示)
                            state
                                .bus_tx
                                .publish(BusEvent::ServerEvent(ServerEvent::Face { position }));

                            // 仅在追踪开启时调整舵机
                            if state.face_tracking_enabled.load(Ordering::Relaxed)
                                && position.has_face
                            {
                                let target = calculate_body_adjustment(position.x);
                                let prev = state.face_tracking_adjustment.load(Ordering::Relaxed);
                                let smoothed = smooth_adjustment(prev, target, 0.3);
                                state
                                    .face_tracking_adjustment
                                    .store(smoothed, Ordering::Relaxed);

                                let current_angle = state.joint.values()[BODY_SERVO_INDEX];
                                let new_angle =
                                    (f32::from(current_angle) + smoothed as f32).clamp(-90.0, 90.0);
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
        self.bus_tx
            .publish(BusEvent::ServerEvent(ServerEvent::Connection {
                is_connected,
            }));
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
        self.bus_tx
            .publish(BusEvent::ServerEvent(ServerEvent::JointState { state }));
    }

    /// 广播当前 JointConfig(用于预览/调试)
    pub fn broadcast_joint_config(&self) {
        self.bus_tx
            .publish(BusEvent::ServerEvent(ServerEvent::JointConfig {
                config: joint_config_to_proto(&self.joint.config()),
            }));
    }

    /// 获取当前 config
    pub fn config(&self) -> AppConfig {
        self.config.read().unwrap().clone()
    }

    /// 更新 config
    ///
    /// 若 `speech_name` / `output_device` 与旧值不同, 立即重建
    /// `VoiceManager` (热生效); 若 `camera_index` 与旧值不同, 立即重建
    /// `VideoCapture` (热生效). 重建失败时 config 仍按本次值持久化
    /// (用户已经表达过选择意图), 但通过 `ServerEvent::Error` 告知
    /// 客户端当前 ASR/TTS/摄像头仍在用旧设备. 视频端在 `rebuild_video`
    /// 失败时**额外**用旧 index 尝试 fallback, 确保视频流不中断.
    pub fn set_config(&self, cfg: AppConfig) -> anyhow::Result<()> {
        let (old_mic, old_spk) = self.current_audio_config();
        let old_cam = self.current_video_config();
        let audio_changed = cfg.speech_name != old_mic || cfg.output_device != old_spk;
        let video_changed = cfg.camera_index != old_cam;
        cfg.save()?;
        *self.config.write().unwrap() = cfg.clone();
        self.bus_tx
            .publish(BusEvent::ServerEvent(ServerEvent::Config {
                config: cfg.clone(),
            }));
        if audio_changed {
            if let Err(e) = self.rebuild_voice() {
                log::warn!("rebuild_voice failed: {e:?}");
                self.bus_tx
                    .publish(BusEvent::ServerEvent(ServerEvent::Error {
                        message: format!("voice rebuild failed: {e}"),
                    }));
            }
        }
        if video_changed {
            if let Err(e) = self.rebuild_video() {
                log::warn!("rebuild_video failed: {e:?}");
                // fallback: 用内存里"用户提交前的旧 index"重建一次, 让视频流不断.
                // 失败也要再尝试 — 第一次失败可能是新设备独占占用, 再 fallback 也可能再失败.
                if !old_cam.is_empty() && old_cam != cfg.camera_index {
                    if let Err(e2) = self.rebuild_with_override(&old_cam) {
                        log::error!("camera rebuild fallback also failed: {e2:?}");
                        self.bus_tx
                            .publish(BusEvent::ServerEvent(ServerEvent::Error {
                                message: format!(
                                    "camera rebuild failed: {e}; fallback also failed: {e2}"
                                ),
                            }));
                    } else {
                        self.bus_tx
                            .publish(BusEvent::ServerEvent(ServerEvent::Error {
                                message: format!(
                                    "camera rebuild failed: {e}; switched back to '{old_cam}'"
                                ),
                            }));
                    }
                } else {
                    self.bus_tx
                        .publish(BusEvent::ServerEvent(ServerEvent::Error {
                            message: format!("camera rebuild failed: {e}"),
                        }));
                }
            } else {
                // 成功: 把新分辨率推给客户端.
                let (w, h) = self.video().map(|v| v.resolution()).unwrap_or((0, 0));
                self.bus_tx
                    .publish(BusEvent::ServerEvent(ServerEvent::CameraResolution {
                        width: w,
                        height: h,
                    }));
            }
        }
        Ok(())
    }
}

impl SharedState {
    /// 用临时 index 重建一次 `VideoCapture`, 不动 `self.config`.
    /// `set_config` 的 fallback 路径专用 — 用旧 index 重建一次, 让视频流不断.
    fn rebuild_with_override(&self, cam_index: &str) -> anyhow::Result<()> {
        let new_index = parse_camera_index(cam_index);
        let rotation = rotate_proto_to_local(self.config.read().unwrap().rotation);

        let old = {
            let mut guard = self.video.lock().unwrap();
            guard.take()
        };
        let mut new_capture = VideoCapture::new(new_index, self.bus_tx.clone(), rotation);
        new_capture.start_capture_frames_thread();
        *self.video.lock().unwrap() = Some(Arc::new(new_capture));
        drop(old);
        Ok(())
    }
}

/// `proto::Mood` -> `boteyes::Mood`
#[must_use]
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

/// `boteyes::Mood` -> `proto::Mood`
#[must_use]
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

/// `proto::RotateAngle` -> 内部 `video::process::RotateAngle`
#[must_use]
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

/// 内部 Action -> `proto::Action`
#[must_use]
pub fn action_to_proto(a: &crate::llm::response::Action) -> ele_bot_proto::Action {
    ele_bot_proto::Action {
        servo_index: a.servo_index,
        angle: a.angle,
        duration_ms: a.duration_ms,
    }
}

/// 内部 `JointConfig` -> `proto::JointConfig`
#[must_use]
pub fn joint_config_to_proto(c: &JointConfig) -> ele_bot_proto::JointConfig {
    ele_bot_proto::JointConfig {
        enable: c.enable,
        angles: c.angles,
    }
}
