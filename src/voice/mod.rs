//! 语音识别模块
//!
//! 使用 Vosk 库实现离线语音识别功能
//! 唤醒词: "BD"

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Stream};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use vosk::{Model, Recognizer};

/// 语音识别状态
#[derive(Clone, Debug, PartialEq)]
pub enum VoiceState {
    Idle,
    Listening,
    Detected(String),
}

/// 语音管理器
///
/// 封装音频流和状态，支持 UI 实时查询音量。
#[allow(dead_code)]
pub struct VoiceManager {
    /// 音频流，保持存活
    _stream: Stream,
    /// 当前音量 (0-100)
    volume: Arc<AtomicI32>,
    /// 当前语音状态
    state: Arc<Mutex<VoiceState>>,
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
        let state = Arc::new(Mutex::new(VoiceState::Idle));
        let (state_tx, state_rx) = mpsc::sync_channel::<VoiceState>(4);

        let recognizer = SpeechRecognizer::new(model_path)?;
        let (audio_tx, audio_rx) = mpsc::sync_channel::<Vec<i16>>(4);

        let volume_clone = volume.clone();
        let state_clone = state.clone();
        let error_handler = |e| log::error!("Audio stream error: {e}");
        let stream = device.build_input_stream(
            &config,
            move |data: &[f32], _: &_| {
                // 计算音量
                let sum: f32 = data.iter().map(|&s| s * s).sum();
                let rms = (sum / data.len() as f32).sqrt();
                let volume = (rms * 100.0).min(100.0) as i32;
                volume_clone.store(volume, Ordering::Relaxed);

                let samples: Vec<i16> =
                    data.iter().map(|&s| (s * i16::MAX as f32) as i16).collect();
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

        // 状态更新线程
        thread::spawn(move || {
            update_state_thread(state_rx, state_clone);
        });

        // 音频分析线程
        thread::spawn(move || {
            audio_analysis_thread(state_tx, recognizer, audio_rx);
        });

        Ok(Self {
            _stream: stream,
            volume,
            state,
        })
    }

    /// 获取当前音量 (0-100)
    pub fn volume(&self) -> i32 {
        self.volume.load(Ordering::Relaxed)
    }

    /// 获取当前语音状态
    pub fn state(&self) -> std::sync::MutexGuard<'_, VoiceState> {
        self.state.lock().unwrap()
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
fn audio_analysis_thread(
    tx: SyncSender<VoiceState>,
    mut recognizer: SpeechRecognizer,
    audio_rx: Receiver<Vec<i16>>,
) {
    let chunk_size = 1600;
    let mut buffer = Vec::new();

    let _ = tx.send(VoiceState::Idle);

    for samples in audio_rx {
        buffer.extend(samples);

        while buffer.len() >= chunk_size {
            let frame = &buffer[0..chunk_size];
            recognizer.process(frame);

            let cur_state = recognizer.state().clone();
            if matches!(cur_state, VoiceState::Detected(_)) {
                if let Err(e) = tx.send(cur_state) {
                    log::warn!("Failed to send voice state: {e}");
                }
            }

            buffer.drain(..chunk_size);
        }
    }
}

/// 语音状态更新线程
fn update_state_thread(rx: Receiver<VoiceState>, state: Arc<Mutex<VoiceState>>) {
    for new_state in rx {
        let mut current = state.lock().unwrap();
        if matches!(new_state, VoiceState::Detected(_)) {
            log::info!("Wake word detected");
            *current = VoiceState::Detected(String::new());
            // 1秒后重置
            thread::sleep(std::time::Duration::from_secs(1));
            *current = VoiceState::Idle;
        }
    }
}

/// 语音识别器
pub struct SpeechRecognizer {
    recognizer: Recognizer,
    state: VoiceState,
}

impl SpeechRecognizer {
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

    pub fn process(&mut self, audio_data: &[i16]) {
        if self.state == VoiceState::Idle {
            self.state = VoiceState::Listening;
        }

        let _ = self.recognizer.accept_waveform(audio_data);

        let partial = self.recognizer.partial_result();
        if !partial.partial.is_empty() {
            let text = partial.partial.to_string();
            let lower = text.to_lowercase();

            if lower.contains("bd") || lower.contains("b d") {
                self.state = VoiceState::Detected(text.clone());
                log::info!("Wake word detected: {text}");
            }
        }
    }

    pub fn state(&self) -> &VoiceState {
        &self.state
    }
}
