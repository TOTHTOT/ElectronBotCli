use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Stream};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::{mpsc, Arc};
use std::thread;
use vosk::{Model, Recognizer};

#[allow(dead_code)]
pub struct VoiceManager {
    _stream: Stream,
    volume: Arc<AtomicI32>,
}

#[allow(dead_code)]
impl VoiceManager {
    pub fn new(
        model_path: &str,
        speech_name: &str,
        result_tx: mpsc::Sender<String>,
    ) -> Result<Self> {
        let device = find_input_device(speech_name)?;
        let config = get_input_config(&device)?;
        let need_resample = config.sample_rate() != 16000;

        let volume = Arc::new(AtomicI32::new(0));
        let recognizer = SpeechRecognizer::new(model_path)?;
        let (audio_tx, audio_rx) = mpsc::sync_channel::<Vec<i16>>(4);

        let stream = build_audio_stream(&device, &config, need_resample, volume.clone(), audio_tx)?;
        stream.play()?;

        thread::spawn(move || {
            audio_analysis_thread(recognizer, audio_rx, result_tx);
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

fn get_input_config(device: &Device) -> Result<cpal::SupportedStreamConfig> {
    device
        .default_input_config()
        .map_err(|e| anyhow!("Failed to get input config: {}", e))
}

fn build_audio_stream(
    device: &Device,
    config: &cpal::SupportedStreamConfig,
    need_resample: bool,
    volume: Arc<AtomicI32>,
    audio_tx: SyncSender<Vec<i16>>,
) -> Result<Stream> {
    let channels = config.channels() as usize;
    let sample_rate = config.sample_rate();
    let volume_clone = volume.clone();

    device
        .build_input_stream(
            &config.clone().into(),
            move |data: &[f32], _: &_| {
                let sum: f32 = data.iter().map(|&s| s * s).sum();
                let rms = (sum / data.len() as f32).sqrt();
                volume_clone.store((rms * 100.0).min(100.0) as i32, Ordering::Relaxed);

                let mono: Vec<f32> = if channels == 2 {
                    data.chunks(2).map(|c| (c[0] + c[1]) / 2.0).collect()
                } else {
                    data.to_vec()
                };

                let samples: Vec<i16> =
                    mono.iter().map(|&s| (s * i16::MAX as f32) as i16).collect();
                let final_samples = if need_resample {
                    resample_to_16k(&samples, sample_rate)
                } else {
                    samples
                };
                let _ = audio_tx.send(final_samples);
            },
            |e| log::error!("Audio stream error: {}", e),
            None,
        )
        .map_err(|e| anyhow!("Failed to build input stream: {}", e))
}

fn resample_to_16k(samples: &[i16], from_rate: u32) -> Vec<i16> {
    let ratio = from_rate as f64 / 16000.0;
    let new_len = (samples.len() as f64 / ratio) as usize;
    (0..new_len)
        .filter_map(|i| samples.get((i as f64 * ratio) as usize).copied())
        .collect()
}

/// 音频解析线程
///
/// # Arguments
///
/// * `recognizer`: 解析器
/// * `audio_rx`: 原始音频数据
/// * `last_text`: 解析结果
///
/// returns: ()
///
/// # Examples
///
/// ```
///
/// ```
fn audio_analysis_thread(
    mut recognizer: SpeechRecognizer,
    audio_rx: Receiver<Vec<i16>>,
    result_tx: mpsc::Sender<String>,
) {
    let chunk_size = 1600;
    let mut buffer = Vec::new();
    for samples in audio_rx {
        buffer.extend(samples);
        while buffer.len() >= chunk_size {
            let frame = &buffer[..chunk_size];
            if let Some(text) = recognizer.process(frame) {
                if !text.is_empty() {
                    log::info!("Audio analysis result: {text}");
                    let _ = result_tx.send(text);
                }
            }
            buffer.drain(..chunk_size);
        }
    }
}

pub struct SpeechRecognizer {
    recognizer: Recognizer,
}

impl SpeechRecognizer {
    pub fn new(model_path: &str) -> Result<Self> {
        let model = Model::new(model_path)
            .ok_or_else(|| anyhow!("Failed to load model: {}", model_path))?;
        let recognizer = Recognizer::new(&model, 16000.0)
            .ok_or_else(|| anyhow!("Failed to create recognizer"))?;
        Ok(Self { recognizer })
    }

    pub fn process(&mut self, audio_data: &[i16]) -> Option<String> {
        let state = self.recognizer.accept_waveform(audio_data).ok()?;
        if matches!(state, vosk::DecodingState::Finalized) {
            let result = self.recognizer.final_result();
            result.single().and_then(|s| {
                let text = s.text.trim().to_string();
                if text.is_empty() {
                    None
                } else {
                    Some(text)
                }
            })
        } else {
            None
        }
    }
}

pub fn play_beep(count: u32, frequency: f32, duration_ms: u32, interval_ms: u32) {
    let device = match get_output_device() {
        Some(d) => d,
        None => return,
    };

    let config = match get_output_config(&device) {
        Some(c) => c,
        None => return,
    };

    let sr: u32 = config.sample_rate();
    let sample_rate = sr as f32;
    let channels = config.channels() as usize;

    let samples = generate_beep_samples(
        count,
        frequency,
        duration_ms,
        interval_ms,
        sample_rate,
        channels,
    );
    play_samples(
        &device,
        &config,
        samples,
        channels,
        (count * duration_ms + (count.saturating_sub(1)) * interval_ms) as u64,
    );
}

fn get_output_device() -> Option<Device> {
    cpal::default_host().default_output_device()
}

fn get_output_config(device: &Device) -> Option<cpal::SupportedStreamConfig> {
    device.default_output_config().ok()
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

fn play_samples(
    device: &Device,
    config: &cpal::SupportedStreamConfig,
    samples: Vec<f32>,
    channels: usize,
    duration_ms: u64,
) {
    let stream = match device.build_output_stream(
        &cpal::StreamConfig {
            channels: channels as cpal::ChannelCount,
            sample_rate: config.sample_rate(),
            buffer_size: cpal::BufferSize::Default,
        },
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            for (i, sample) in data.iter_mut().enumerate() {
                *sample = samples.get(i).copied().unwrap_or(0.0);
            }
        },
        |err| log::error!("Beep stream error: {}", err),
        None,
    ) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("Failed to build beep stream: {}", e);
            return;
        }
    };

    if let Err(e) = stream.play() {
        log::warn!("Failed to play beep: {}", e);
        return;
    }
    thread::sleep(std::time::Duration::from_millis(duration_ms + 50));
}
