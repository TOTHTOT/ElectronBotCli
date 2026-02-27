mod asr;
mod tts;

use crate::voice::asr::{build_audio_stream, recognition_thread};
use anyhow::{anyhow, bail, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Stream};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
pub const VAD_WINDOW_SIZE: i32 = 512;
pub const CHUNK_SIZE: usize = 1600; // 100ms at 16kHz
pub const SAMPLE_RATE: u32 = 16000;
#[allow(dead_code)]
pub struct VoiceManager {
    _stream: Stream,
    volume: Arc<AtomicI32>,
}

#[allow(dead_code)]
impl VoiceManager {
    /// 创建voice模块, 通过静音检测截取有效实时音频数据,
    /// 完成后发送到解析线程
    ///
    /// # Arguments
    ///
    /// * `model_path`:
    /// * `_tts_model_path`:
    /// * `_tts_tokens_path`:
    /// * `speech_name`:
    /// * `result_tx`:
    ///
    /// returns: Result<VoiceManager, Error>
    ///
    /// # Examples
    ///
    /// ```
    ///
    /// ```
    pub fn new(
        model_path: &str,
        _tts_model_path: &str,
        _tts_tokens_path: &str,
        speech_name: &str,
        result_tx: mpsc::Sender<String>,
    ) -> Result<Self> {
        let device = find_input_device(speech_name)?; // 查找输入麦克风
        let volume = Arc::new(AtomicI32::new(0)); // 实时音量
                                                  // 原始音频数据传输通道
        let (audio_tx, audio_rx) = mpsc::sync_channel::<Vec<f32>>(4);
        let stream = build_audio_stream(&device, volume.clone(), audio_tx)?;
        stream.play()?;

        let model_path_owned = model_path.to_string();
        thread::spawn(move || {
            recognition_thread(&model_path_owned, audio_rx, result_tx);
        });

        Ok(Self {
            _stream: stream,
            volume,
        })
    }

    pub fn volume(&self) -> i32 {
        self.volume.load(Ordering::Relaxed)
    }
}

fn find_input_device(speech_name: &str) -> Result<Device> {
    let host = cpal::default_host();
    let devices: Vec<_> = host
        .input_devices()?
        .filter_map(|d| {
            d.description()
                .ok()
                .map(|desc| (desc.name().to_string(), d))
        })
        .collect();

    devices
        .iter()
        .find(|(name, _)| name == speech_name)
        .map(|(_, d)| d.clone())
        .ok_or_else(|| anyhow!("No audio input device found: {}", speech_name))
}

/// Play a simple beep sound (for notifications)
pub fn play_beep(count: u32, frequency: f32, duration_ms: u32, interval_ms: u32) {
    let device = match cpal::default_host().default_output_device() {
        Some(d) => d,
        None => return,
    };

    let config = match device.default_output_config() {
        Ok(c) => c,
        Err(_) => return,
    };

    let sample_rate = config.sample_rate() as f32;
    let _channels = config.channels() as usize;

    let samples = generate_beep_samples(
        count,
        frequency,
        duration_ms,
        interval_ms,
        sample_rate,
        _channels,
    );

    let _ = play_output_samples(&device, &config.into(), samples, _channels, duration_ms);
}

fn generate_beep_samples(
    count: u32,
    frequency: f32,
    duration_ms: u32,
    interval_ms: u32,
    sample_rate: f32,
    channels: usize,
) -> Vec<f32> {
    let total_ms = (count * duration_ms) + ((count.saturating_sub(1)) * interval_ms);
    let total_samples = ((sample_rate * total_ms as f32) / 1000.0) as usize * channels;
    let mut samples = vec![0.0f32; total_samples];

    for i in 0..count {
        let start =
            ((sample_rate * (i * (duration_ms + interval_ms)) as f32) / 1000.0) as usize * channels;
        let count = ((sample_rate * duration_ms as f32) / 1000.0) as usize * channels;
        for j in 0..count {
            let t = (j / channels) as f32 / sample_rate;
            let sine = (2.0 * std::f32::consts::PI * frequency * t).sin();
            let env = if j < count / 4 {
                j as f32 / (count / 4) as f32
            } else if j > count * 3 / 4 {
                (count - j) as f32 / (count / 4) as f32
            } else {
                1.0
            };
            samples[start + j] = sine * env * 0.5;
        }
    }
    samples
}

fn play_output_samples(
    device: &Device,
    config: &cpal::StreamConfig,
    samples: Vec<f32>,
    _channels: usize,
    duration_ms: u32,
) -> Result<(), anyhow::Error> {
    let stream = device.build_output_stream(
        config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            for (i, sample) in data.iter_mut().enumerate() {
                *sample = samples.get(i).copied().unwrap_or(0.0);
            }
        },
        |err| log::error!("Beep stream error: {}", err),
        None,
    )?;

    stream.play()?;
    thread::sleep(std::time::Duration::from_millis(duration_ms as u64 + 50));

    Ok(())
}
