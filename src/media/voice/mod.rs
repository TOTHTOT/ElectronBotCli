pub mod asr;
mod tts;

use crate::media::voice::asr::{build_audio_stream, recognition_thread};
use anyhow::{anyhow, Result};
use boteyes::Mood;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Stream};
use std::fs::File;
use std::path::Path;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;

pub const VAD_WINDOW_SIZE: i32 = 512;
#[allow(dead_code)]
pub const CHUNK_SIZE: usize = 1600; // 100ms at 16kHz
#[allow(dead_code)]
pub const SAMPLE_RATE: u32 = 16000;
#[allow(dead_code)]
pub struct VoiceManager {
    _stream: Stream,
    volume: Arc<AtomicI32>,
    pub rx: mpsc::Receiver<String>,
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
        sense_voice_model_path: impl AsRef<Path>,
        silero_vad_model_path: impl AsRef<Path>,
        tokens_path: impl AsRef<Path>,
        speech_name: &str,
    ) -> Result<Self> {
        let device = find_input_device(speech_name)?; // 查找输入麦克风
        let volume = Arc::new(AtomicI32::new(0)); // 实时音量
        let (audio_tx, audio_rx) = mpsc::sync_channel::<Vec<f32>>(4); // 原始音频数据传输通道
        let stream = build_audio_stream(&device, volume.clone(), audio_tx)?;
        stream.play()?;
        let sense_voice_model_path = sense_voice_model_path.as_ref().into();
        let silero_vad_model_path = silero_vad_model_path.as_ref().into();
        let tokens_path = tokens_path.as_ref().into();

        // 创建解析音频线程, 结果提供 text_rx 传递
        let (text_tx, text_rx) = mpsc::channel::<String>();
        thread::spawn(move || {
            if let Err(e) = recognition_thread(
                sense_voice_model_path,
                silero_vad_model_path,
                tokens_path,
                audio_rx,
                text_tx,
            ) {
                log::error!("recognition_thread failed: {e:?}");
            }
        });

        Ok(Self {
            _stream: stream,
            volume,
            rx: text_rx,
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
    log::info!(
        "input audio device: {:#?}",
        devices.iter().map(|(name, _)| name).collect::<Vec<_>>()
    );
    let device = devices
        .iter()
        .find(|(name, _)| name.contains(speech_name))
        .map(|(_, d)| d.clone())
        .ok_or_else(|| anyhow!("No audio input device found containing: {}", speech_name))?;

    // 打印设备配置信息
    if let Ok(config) = device.default_input_config() {
        log::info!("Selected audio device config: {config:?} ");
    }

    Ok(device)
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

/// BD1 电子机器人音风格参数
#[derive(Debug, Clone, Copy)]
pub struct Bd1SoundParams {
    pub base_freq: f32,
    pub freq_range: f32,
    pub duration_ms: u32,
    pub beep_count: u32,
    pub interval_ms: u32,
    pub sweep_direction: SweepDirection,
    pub harmonic_ratio: f32,
}

#[derive(Debug, Clone, Copy)]
pub enum SweepDirection {
    Up,
    Down,
    None,
}

impl Bd1SoundParams {
    pub fn for_mood(mood: Mood) -> Self {
        match mood {
            Mood::Happy => Self {
                base_freq: 800.0,
                freq_range: 400.0,
                duration_ms: 80,
                beep_count: 3,
                interval_ms: 100,
                sweep_direction: SweepDirection::Up,
                harmonic_ratio: 0.3,
            },
            Mood::Surprise => Self {
                base_freq: 600.0,
                freq_range: 400.0,
                duration_ms: 100,
                beep_count: 2,
                interval_ms: 150,
                sweep_direction: SweepDirection::Up,
                harmonic_ratio: 0.25,
            },
            Mood::Angry => Self {
                base_freq: 200.0,
                freq_range: 200.0,
                duration_ms: 120,
                beep_count: 2,
                interval_ms: 80,
                sweep_direction: SweepDirection::Down,
                harmonic_ratio: 0.4,
            },
            Mood::Sad => Self {
                base_freq: 150.0,
                freq_range: 150.0,
                duration_ms: 200,
                beep_count: 1,
                interval_ms: 0,
                sweep_direction: SweepDirection::Down,
                harmonic_ratio: 0.2,
            },
            Mood::Confuse => Self {
                base_freq: 400.0,
                freq_range: 200.0,
                duration_ms: 80,
                beep_count: 4,
                interval_ms: 60,
                sweep_direction: SweepDirection::None,
                harmonic_ratio: 0.35,
            },
            Mood::Default => Self {
                base_freq: 440.0,
                freq_range: 440.0,
                duration_ms: 100,
                beep_count: 2,
                interval_ms: 120,
                sweep_direction: SweepDirection::Up,
                harmonic_ratio: 0.3,
            },
            Mood::Loading => Self {
                base_freq: 660.0,
                freq_range: 0.0,
                duration_ms: 300,
                beep_count: 1,
                interval_ms: 0,
                sweep_direction: SweepDirection::None,
                harmonic_ratio: 0.1,
            },
        }
    }
}

/// 生成 BD1 风格的音频样本（带扫频和多频率成分）
pub fn generate_bd1_samples(mood: Mood) -> Vec<f32> {
    let params = Bd1SoundParams::for_mood(mood);
    let sample_rate = 44100.0;
    let channels = 1usize;

    let total_ms =
        (params.beep_count * params.duration_ms) + ((params.beep_count.saturating_sub(1)) * params.interval_ms);
    let total_samples = ((sample_rate * total_ms as f32) / 1000.0) as usize * channels;
    let mut samples = vec![0.0f32; total_samples];

    for i in 0..params.beep_count {
        let start =
            ((sample_rate * (i * (params.duration_ms + params.interval_ms)) as f32) / 1000.0) as usize
                * channels;
        let beep_samples = ((sample_rate * params.duration_ms as f32) / 1000.0) as usize * channels;

        for j in 0..beep_samples {
            let t = j as f32 / sample_rate;
            let progress = j as f32 / beep_samples as f32;

            // 计算当前频率（扫频效果）
            let freq = match params.sweep_direction {
                SweepDirection::Up => params.base_freq + (params.freq_range * progress),
                SweepDirection::Down => params.base_freq + (params.freq_range * (1.0 - progress)),
                SweepDirection::None => params.base_freq,
            };

            // 基频
            let fundamental = (2.0 * std::f32::consts::PI * freq * t).sin();

            // 二次谐波
            let harmonic2 = (2.0 * std::f32::consts::PI * freq * 2.0 * t).sin() * params.harmonic_ratio;

            // 三次谐波（更柔和）
            let harmonic3 = (2.0 * std::f32::consts::PI * freq * 3.0 * t).sin() * params.harmonic_ratio * 0.5;

            // 包络（attack-decay）
            let env = if progress < 0.1 {
                progress / 0.1
            } else if progress > 0.8 {
                (1.0 - progress) / 0.2
            } else {
                1.0
            };

            // 混合所有频率成分
            samples[start + j] = (fundamental + harmonic2 + harmonic3) * env * 0.4;
        }
    }

    samples
}

/// 将样本写入 WAV 文件
pub fn write_wav_file(path: &Path, samples: &[f32], sample_rate: u32) -> std::io::Result<()> {
    use std::io::Write;

    let bits_per_sample = 16u16;
    let num_channels = 1u16;
    let byte_rate = sample_rate as u32 * num_channels as u32 * bits_per_sample as u32 / 8;
    let block_align = num_channels * bits_per_sample / 8;
    let data_size = samples.len() * 2; // 16-bit = 2 bytes per sample

    let mut file = File::create(path)?;

    // RIFF header
    file.write_all(b"RIFF")?;
    file.write_all(&(36 + data_size as u32).to_le_bytes())?;
    file.write_all(b"WAVE")?;

    // fmt chunk
    file.write_all(b"fmt ")?;
    file.write_all(&16u32.to_le_bytes())?; // chunk size
    file.write_all(&1u16.to_le_bytes())?; // audio format (PCM)
    file.write_all(&num_channels.to_le_bytes())?;
    file.write_all(&sample_rate.to_le_bytes())?;
    file.write_all(&byte_rate.to_le_bytes())?;
    file.write_all(&block_align.to_le_bytes())?;
    file.write_all(&bits_per_sample.to_le_bytes())?;

    // data chunk
    file.write_all(b"data")?;
    file.write_all(&(data_size as u32).to_le_bytes())?;

    // Write samples as 16-bit PCM
    for sample in samples {
        let s = (*sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
        file.write_all(&s.to_le_bytes())?;
    }

    Ok(())
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

/// 播放 WAV 文件
pub fn play_wav_file(path: &Path) -> Result<(), anyhow::Error> {
    let device = match cpal::default_host().default_output_device() {
        Some(d) => d,
        None => return Err(anyhow!("No output device found")),
    };

    let config = match device.default_output_config() {
        Ok(c) => c,
        Err(e) => return Err(anyhow!("Failed to get output config: {}", e)),
    };

    // 读取 WAV 文件
    let samples = read_wav_samples(path)?;

    let sample_rate = config.sample_rate() as f32;
    let channels = config.channels() as usize;
    let duration_ms = (samples.len() as f32 / sample_rate * 1000.0) as u32;

    let samples: Vec<f32> = if channels > 1 {
        samples.iter().copied().collect()
    } else {
        samples
    };

    play_output_samples(&device, &config.into(), samples, channels, duration_ms)
}

/// 从 WAV 文件读取样本
fn read_wav_samples(path: &Path) -> Result<Vec<f32>, anyhow::Error> {
    use std::io::Read;

    let mut file = File::open(path)?;
    let metadata = file.metadata()?;
    let mut header = [0u8; 44];
    file.read_exact(&mut header)?;

    // 验证 RIFF header
    if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return Err(anyhow!("Invalid WAV file"));
    }

    let _sample_rate = u32::from_le_bytes([header[24], header[25], header[26], header[27]]);
    let bits_per_sample = u16::from_le_bytes([header[34], header[35]]);
    let num_channels = u16::from_le_bytes([header[22], header[23]]);

    let bytes_per_sample = bits_per_sample as usize / 8;
    let data_size = (metadata.len() - 44) as usize;
    let total_samples = data_size / bytes_per_sample;
    let mut samples = Vec::with_capacity(total_samples);

    for _ in 0..total_samples {
        let mut buf = vec![0u8; bytes_per_sample];
        file.read_exact(&mut buf)?;
        let sample = match bits_per_sample {
            16 => {
                let s = i16::from_le_bytes([buf[0], buf[1]]);
                s as f32 / 32768.0
            }
            8 => {
                let s = buf[0] as i8;
                s as f32 / 128.0
            }
            _ => return Err(anyhow!("Unsupported bit depth: {}", bits_per_sample)),
        };
        samples.push(sample);
    }

    // 如果是双声道，只取第一个声道
    if num_channels > 1 {
        samples = samples.iter().copied().step_by(num_channels as usize).collect();
    }

    Ok(samples)
}

/// 生成 BD1 音效并保存为 WAV 文件，然后播放
pub fn play_bd1_sound(mood: Mood) {
    let temp_dir = std::env::temp_dir();
    let filename = format!("bd1_{:?}.wav", mood);
    let wav_path = temp_dir.join(filename);

    // 生成样本
    let samples = generate_bd1_samples(mood);
    let sample_rate = 44100;

    // 写入 WAV 文件
    if let Err(e) = write_wav_file(&wav_path, &samples, sample_rate) {
        log::error!("Failed to write WAV file: {}", e);
        return;
    }

    log::info!("Generated BD1 sound WAV: {:?}", wav_path);

    // 播放 WAV 文件
    if let Err(e) = play_wav_file(&wav_path) {
        log::error!("Failed to play WAV file: {}", e);
    }
}
