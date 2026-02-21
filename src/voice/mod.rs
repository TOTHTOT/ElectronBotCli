use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Stream};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::{mpsc, Arc};
use std::thread;
use vosk::{Model, Recognizer};

/// 语音唤醒事件
#[derive(Clone, Debug)]
pub struct WakeEvent {
    pub text: String,
}

/// 语音管理器
///
/// 封装音频流和 Vosk 识别器
#[allow(dead_code)]
pub struct VoiceManager {
    _stream: Stream,
    volume: Arc<AtomicI32>,
}

#[allow(dead_code)]
impl VoiceManager {
    /// 创建语音管理器
    pub fn new(model_path: &str, speech_name: &str) -> Result<Self> {
        // 获取音频设备列表
        let devices = list_devices();
        for (name, _) in &devices {
            log::info!("find speech: {name}");
        }

        // 查找指定麦克风
        let (device_name, device) = devices
            .into_iter()
            .find(|(name, _)| name == speech_name)
            .ok_or_else(|| anyhow!("No audio input device found: {speech_name}"))?;

        log::info!("Using audio device: {device_name}");

        // 获取设备的默认配置
        let default_config = device.default_input_config()?;
        let actual_sample_rate = default_config.sample_rate();
        let actual_channels = default_config.channels();
        log::info!("Device sample rate: {actual_sample_rate} Hz, channels: {actual_channels}");

        let need_resample = actual_sample_rate != 16000;

        let config = cpal::StreamConfig {
            channels: actual_channels,
            sample_rate: default_config.sample_rate(),
            buffer_size: cpal::BufferSize::Default,
        };

        // 共享状态
        let volume = Arc::new(AtomicI32::new(0));
        let (wake_tx, wake_rx) = mpsc::sync_channel::<WakeEvent>(4);

        let recognizer = SpeechRecognizer::new(model_path)?;
        let (audio_tx, audio_rx) = mpsc::sync_channel::<Vec<i16>>(4);

        let volume_clone = volume.clone();
        let error_handler = |e| log::error!("Audio stream error: {e}");
        let stream = device.build_input_stream(
            &config,
            move |data: &[f32], _: &_| {
                // 计算音量
                let sum: f32 = data.iter().map(|&s| s * s).sum();
                let rms = (sum / data.len() as f32).sqrt();
                let volume = (rms * 100.0).min(100.0) as i32;
                volume_clone.store(volume, Ordering::Relaxed);

                // 双声道混合成单声道
                let mono_samples: Vec<f32> = if actual_channels == 2 {
                    data.chunks(2)
                        .map(|chunk| (chunk[0] + chunk[1]) / 2.0)
                        .collect()
                } else {
                    data.to_vec()
                };

                // 转换为 i16
                let samples: Vec<i16> = mono_samples
                    .iter()
                    .map(|&s| (s * i16::MAX as f32) as i16)
                    .collect();

                // 重采样到 16kHz
                let final_samples = if need_resample {
                    resample_to_16k(&samples, actual_sample_rate)
                } else {
                    samples
                };
                let _ = audio_tx.send(final_samples);
            },
            error_handler,
            None,
        )?;
        stream.play()?;
        log::info!("Voice recognition thread started");

        thread::spawn(move || {
            audio_analysis_thread(wake_tx, recognizer, audio_rx);
        });

        thread::spawn(move || {
            for event in wake_rx {
                log::trace!("Wake event: {:?}", event);
                if SpeechRecognizer::is_wake_word(&event.text) {
                    log::info!("Wake word detected");
                }
            }
        });

        Ok(Self {
            _stream: stream,
            volume,
        })
    }

    /// 获取当前音量 (0-100)
    pub fn volume(&self) -> i32 {
        self.volume.load(Ordering::Relaxed)
    }
}

/// 列出所有可用的音频输入设备
fn list_devices() -> Vec<(String, Device)> {
    let host = cpal::default_host();
    let mut devices = Vec::new();

    if let Ok(iter) = host.input_devices() {
        for device in iter {
            if let Ok(desc) = device.description() {
                devices.push((desc.name().to_string(), device));
            }
        }
    }

    devices
}

/// 将音频重采样到 16kHz
///
/// # Arguments
///
/// * `samples`:
/// * `from_rate`:
///
/// returns: Vec<i16, Global>
///
/// # Examples
///
/// ```
///
/// ```
fn resample_to_16k(samples: &[i16], from_rate: u32) -> Vec<i16> {
    let ratio = from_rate as f64 / 16000.0;
    let new_len = (samples.len() as f64 / ratio) as usize;
    let mut result = Vec::with_capacity(new_len);

    for i in 0..new_len {
        let src_idx = (i as f64 * ratio) as usize;
        if src_idx < samples.len() {
            result.push(samples[src_idx]);
        }
    }

    result
}

/// 音频分析线程
///
/// # Arguments
///
/// * `wake_tx`:
/// * `recognizer`:
/// * `audio_rx`:
///
/// returns: ()
///
/// # Examples
///
/// ```
///
/// ```
fn audio_analysis_thread(
    wake_tx: SyncSender<WakeEvent>,
    mut recognizer: SpeechRecognizer,
    audio_rx: mpsc::Receiver<Vec<i16>>,
) {
    let chunk_size = 1600;
    let mut buffer = Vec::new();

    for samples in audio_rx {
        buffer.extend(samples);

        while buffer.len() >= chunk_size {
            let frame = &buffer[0..chunk_size];
            if let Some(text) = recognizer.process(frame) {
                if text.is_empty() {
                    continue;
                }
                let event = WakeEvent { text };
                if let Err(e) = wake_tx.send(event) {
                    log::warn!("Failed to send wake event: {e}");
                }
            }
            buffer.drain(..chunk_size);
        }
    }
}

/// 语音识别器
pub struct SpeechRecognizer {
    recognizer: Recognizer,
}

impl SpeechRecognizer {
    pub fn new(model_path: &str) -> Result<Self> {
        let model =
            Model::new(model_path).ok_or_else(|| anyhow!("Failed to load model: {model_path}"))?;

        let recognizer = Recognizer::new(&model, 16000.0)
            .ok_or_else(|| anyhow!("Failed to create recognizer"))?;

        Ok(Self { recognizer })
    }

    /// 处理音频数据，返回识别到的文本
    ///
    /// # Arguments
    ///
    /// * `audio_data`:
    ///
    /// returns: Option<String>
    ///
    /// # Examples
    ///
    /// ```
    ///
    /// ```
    pub fn process(&mut self, audio_data: &[i16]) -> Option<String> {
        let state = self.recognizer.accept_waveform(audio_data).ok()?;
        if matches!(state, vosk::DecodingState::Finalized) {
            let result = self.recognizer.final_result();
            if let Some(single) = result.single() {
                let text = single.text.trim().to_string();
                if !text.is_empty() {
                    return Some(text);
                }
            }
        }
        None
    }

    /// 检测是否包含唤醒词
    ///
    /// # Arguments
    ///
    /// * `text`:
    ///
    /// returns: bool
    ///
    /// # Examples
    ///
    /// ```
    ///
    /// ```
    pub fn is_wake_word(text: &str) -> bool {
        let lower = text.to_lowercase();

        if lower.contains("小波") {
            return true;
        }

        // 常见误识别变体
        let variants = ["晓波", "小博", "笑波", "晓博"];
        for v in &variants {
            if lower.contains(v) {
                return true;
            }
        }

        false
    }
}
