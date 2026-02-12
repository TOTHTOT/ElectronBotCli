//! 语音识别模块
//!
//! 使用 Vosk 库实现离线语音识别功能
//! 唤醒词: "BD"
//!
//! # 架构
//! - `SpeechRecognizer`: 语音识别器，处理音频流并检测唤醒词
//! - `start_thread()`: 启动语音识别线程
//!
//! # 使用示例
//! ```ignore
//! let (tx, rx) = mpsc::channel::<voice::VoiceState>();
//! let _handle = voice::start_thread(tx, "model")?;
//!
//! for state in rx {
//!     match state {
//!         voice::VoiceState::Detected(w) => println!("唤醒: {w}"),
//!         _ => {}
//!     }
//! }
//! ```

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::Device;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender};
use std::thread;
use vosk::{Model, Recognizer};

/// 语音识别状态
///
/// - `Idle`: 空闲状态，等待唤醒
/// - `Listening`: 正在监听
/// - `Detected(text)`: 检测到唤醒词 "BD"
#[derive(Clone, Debug, PartialEq)]
pub enum VoiceState {
    Idle,
    Listening,
    Detected(String),
}

/// 语音识别器
///
/// 使用 Vosk 库进行流式语音识别，自动检测唤醒词 "BD"。
pub struct SpeechRecognizer {
    /// Vosk 识别器实例
    recognizer: Recognizer,
    /// 当前识别状态
    state: VoiceState,
}

#[allow(dead_code)]
impl SpeechRecognizer {
    /// 创建语音识别器
    ///
    /// # 参数
    /// * `model_path` - Vosk 模型文件所在目录路径
    ///
    /// # 返回
    /// 成功返回识别器实例，失败返回错误信息
    ///
    /// # 示例
    /// ```ignore
    /// let recognizer = SpeechRecognizer::new("model")?;
    /// ```
    pub fn new(model_path: &str) -> Result<Self> {
        let model =
            Model::new(model_path).ok_or_else(|| anyhow!("Failed to load model: {model_path}"))?;

        let recognizer = Recognizer::new(&model, 16000.0)
            .ok_or_else(|| anyhow!("Failed to create recognizer"))?;

        Ok(Self {
            recognizer,
            state: VoiceState::Idle,
        })
    }

    /// 使用默认模型创建识别器
    ///
    /// 尝试按以下顺序查找模型：
    /// 1. `model` 目录
    /// 2. `vosk-model-cn-0.22` 目录
    /// 3. `vosk-model-en-us-0.22` 目录
    pub fn with_default_model() -> Result<Self> {
        for path in ["model", "vosk-model-cn-0.22", "vosk-model-en-us-0.22"] {
            if let Ok(recognizer) = Self::new(path) {
                log::info!("Loaded model: {path}");
                return Ok(recognizer);
            }
        }
        Err(anyhow!("Model not found"))
    }

    /// 处理一帧音频数据
    ///
    /// 将音频数据送入识别器进行流式识别，
    /// 如果检测到唤醒词 "BD" 或 "B D"，状态会变为 `Detected`。
    ///
    /// # 参数
    /// * `audio_data` - PCM 16kHz 单声道音频数据
    pub fn process(&mut self, audio_data: &[i16]) {
        // 首次调用时切换到监听状态
        if self.state == VoiceState::Idle {
            self.state = VoiceState::Listening;
        }

        // 将音频数据送入识别器
        let _ = self.recognizer.accept_waveform(audio_data);

        // 获取部分识别结果
        let partial = self.recognizer.partial_result();
        if !partial.partial.is_empty() {
            let text = partial.partial.to_string();
            let lower = text.to_lowercase();

            // 检测唤醒词 "BD" 或 "B D"
            if lower.contains("bd") || lower.contains("b d") {
                self.state = VoiceState::Detected(text.clone());
                log::info!("Wake word detected: {text}");
            }
        }
    }

    /// 获取当前识别状态
    ///
    /// # 返回
    /// 当前的 `VoiceState`
    pub fn state(&self) -> &VoiceState {
        &self.state
    }

    /// 重置识别状态
    ///
    /// 将状态重置为 `Idle`，下次检测到音频时会重新开始监听
    pub fn reset(&mut self) {
        self.state = VoiceState::Idle;
    }
}

/// 列出所有可用的音频输入设备
///
/// # 返回
/// 返回包含设备名称和设备实例的向量
fn list_devices() -> Vec<(String, Device)> {
    let host = cpal::default_host();
    let mut devices = Vec::new();

    if let Ok(iter) = host.input_devices() {
        for device in iter {
            if let Ok(name) = device.name() {
                devices.push((name, device));
            }
        }
    }

    devices
}

/// 启动语音识别线程
///
/// 创建一个新线程，用于：
/// 1. 从麦克风采集音频数据
/// 2. 使用 Vosk 进行实时语音识别
/// 3. 检测唤醒词 "BD"
/// 4. 通过通道发送识别状态
///
/// # 参数
/// * `tx` - 用于发送识别状态的通道发送端
/// * `model_path` - Vosk 模型文件所在目录路径
///
/// # 返回
/// 成功返回线程句柄，失败返回错误信息
///
/// # 使用示例
/// ```ignore
/// use std::sync::mpsc;
///
/// let (tx, rx) = mpsc::channel::<voice::VoiceState>();
/// let _handle = voice::start_thread(tx, "model")?;
///
/// // 在主线程中接收状态
/// for state in rx {
///     match state {
///         voice::VoiceState::Detected(text) => {
///             println!("唤醒: {}", text);
///         }
///         voice::VoiceState::Listening => {
///             println!("正在监听...");
///         }
///         voice::VoiceState::Idle => {
///             println!("空闲");
///         }
///     }
/// }
/// ```
pub fn start_thread(tx: Sender<VoiceState>, model_path: &str) -> Result<thread::JoinHandle<()>> {
    // 获取音频设备列表
    let devices = list_devices();

    // 选择第一个设备
    let (device_name, device) = devices
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("No audio input device found"))?;

    log::info!("Using audio device: {device_name}");

    // 配置音频参数：16kHz 单声道
    let config = cpal::StreamConfig {
        channels: 1,
        sample_rate: cpal::SampleRate(16000),
        buffer_size: cpal::BufferSize::Default,
    };

    // 创建语音识别器
    let recognizer = SpeechRecognizer::new(model_path)?;

    // 创建音频数据传输通道
    let (audio_tx, audio_rx) = mpsc::sync_channel::<Vec<i16>>(4);

    // 音频流错误处理回调
    let error_handler = |e| log::error!("Audio stream error: {e}");

    // 创建音频输入流
    let stream = device.build_input_stream(
        &config,
        move |data: &[f32], _: &_| {
            // 将 f32 音频数据转换为 i16
            let samples: Vec<i16> = data.iter().map(|&s| (s * i16::MAX as f32) as i16).collect();

            // 发送到处理线程
            let _ = audio_tx.send(samples);
        },
        error_handler,
        None,
    )?;

    // 启动音频流
    stream.play()?;
    log::info!("Voice recognition thread started");

    // 启动音频处理线程
    let handle = thread::spawn(move || {
        audio_analysis_thread(tx, recognizer, audio_rx);
    });

    Ok(handle)
}

/// 音频分析线程
///
/// 从音频通道接收数据，分帧处理，检测唤醒词。
///
/// # 参数
/// * `tx` - 状态发送通道
/// * `recognizer` - 语音识别器实例
/// * `audio_rx` - 音频数据接收通道
fn audio_analysis_thread(
    tx: Sender<VoiceState>,
    mut recognizer: SpeechRecognizer,
    audio_rx: Receiver<Vec<i16>>,
) {
    // Vosk 每帧需要约 1600 样本 (100ms @ 16kHz)
    let chunk_size = 1600;
    let mut buffer = Vec::new();

    // 发送初始状态
    let _ = tx.send(VoiceState::Idle);

    // 持续接收音频数据
    for samples in audio_rx {
        buffer.extend(samples);

        // 处理完整帧
        while buffer.len() >= chunk_size {
            // 取出一帧数据
            let frame = &buffer[0..chunk_size];
            recognizer.process(frame);

            // 如果检测到唤醒词，发送状态
            let cur_state = recognizer.state().clone();
            if matches!(cur_state, VoiceState::Detected(_)) {
                if let Err(e) = tx.send(cur_state) {
                    log::warn!("Failed to send voice state: {e}");
                }
            }

            // 移除已处理的帧
            buffer.drain(..chunk_size);
        }
    }
}
