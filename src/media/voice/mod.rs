pub mod asr;
mod tts;

use crate::media::voice::asr::{build_audio_stream, recognition_thread};
use anyhow::{anyhow, Result};
use boteyes::Mood;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use droid_chatter::{setup_sounds, DroidChatter, Mood as DroidMood};
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
    _stream: cpal::Stream,
    volume: Arc<AtomicI32>,
    pub rx: mpsc::Receiver<String>,
}

#[allow(dead_code)]
impl VoiceManager {
    pub fn new(
        sense_voice_model_path: impl AsRef<Path>,
        silero_vad_model_path: impl AsRef<Path>,
        tokens_path: impl AsRef<Path>,
        speech_name: &str,
    ) -> Result<Self> {
        let device = find_input_device(speech_name)?;
        let volume = Arc::new(AtomicI32::new(0));
        let (audio_tx, audio_rx) = mpsc::sync_channel::<Vec<f32>>(4);
        let stream = build_audio_stream(&device, volume.clone(), audio_tx)?;
        stream.play()?;
        let sense_voice_model_path = sense_voice_model_path.as_ref().into();
        let silero_vad_model_path = silero_vad_model_path.as_ref().into();
        let tokens_path = tokens_path.as_ref().into();

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

fn find_input_device(speech_name: &str) -> Result<cpal::Device> {
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

    if let Ok(config) = device.default_input_config() {
        log::info!("Selected audio device config: {config:?} ");
    }

    Ok(device)
}

/// 使用 droid-chatter 库播放 BD1 机器人声音（使用 cpal 播放，避免 rodio 的 stderr 输出）
pub fn play_bd1_sound(mood: Mood) {
    let temp_dir = std::env::temp_dir();
    let sounds_dir = temp_dir.join("droid_sounds");

    // 首次调用时下载声音文件
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        if let Err(e) = setup_sounds(&sounds_dir) {
            log::error!("Failed to setup droid sounds: {}", e);
        }
    });

    let chatter = match DroidChatter::new(&sounds_dir) {
        Ok(c) => c,
        Err(e) => {
            log::error!("Failed to create DroidChatter: {}", e);
            return;
        }
    };

    // 将 boteyes::Mood 转换为 droid_chatter::Mood
    let droid_mood = match mood {
        Mood::Happy => DroidMood::Happy,
        Mood::Sad => DroidMood::Sad,
        Mood::Angry => DroidMood::Angry,
        _ => DroidMood::Happy,
    };

    // 获取音频数据
    let audio_data = match chatter.bd1_audio("Hello i am bd1", droid_mood) {
        Ok(a) => a,
        Err(e) => {
            log::error!("Failed to get BD1 audio: {}", e);
            return;
        }
    };

    // 保存为项目目录的 WAV 文件（用于调试）
    let wav_path = Path::new("bd1_sounds").join(format!("bd1_{:?}.wav", mood));
    if let Err(e) = write_audio_to_wav(&audio_data, &wav_path) {
        log::error!("Failed to write WAV file: {}", e);
    } else {
        log::info!("Generated BD1 sound WAV: {:?}", wav_path);
    }

    // 使用 cpal 播放音频（避免 rodio 的 stderr 错误输出）
    if let Err(e) = play_audio_with_cpal(&audio_data) {
        log::error!("Failed to play audio: {}", e);
    }
}

/// 使用 cpal 播放音频数据
fn play_audio_with_cpal(audio_data: &droid_chatter::AudioData) -> Result<()> {
    use cpal::traits::{DeviceTrait, StreamTrait};
    use std::sync::Arc;

    let device = cpal::default_host()
        .default_output_device()
        .ok_or_else(|| anyhow!("No output device found"))?;

    let config = device.default_output_config()?;
    let sample_rate = config.sample_rate();
    let _channels = config.channels() as usize;

    // 将 i16 样本转换为 f32
    let samples: Vec<f32> = audio_data
        .samples
        .iter()
        .map(|&s| s as f32 / 32768.0)
        .collect();

    let total_duration_ms = (samples.len() as f32 / sample_rate as f32 * 1000.0) as u64;

    let samples = Arc::new(samples);
    let samples_play = samples.clone();

    let stream = device.build_output_stream(
        &config.into(),
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            for (i, sample) in data.iter_mut().enumerate() {
                *sample = samples_play.get(i).copied().unwrap_or(0.0);
            }
        },
        |err| log::error!("Audio stream error: {}", err),
        None,
    )?;

    stream.play()?;
    thread::sleep(std::time::Duration::from_millis(total_duration_ms + 50));
    drop(stream);

    Ok(())
}

fn write_audio_to_wav(audio_data: &droid_chatter::AudioData, path: &Path) -> Result<()> {
    use hound::{SampleFormat, WavSpec, WavWriter};

    let spec = WavSpec {
        channels: audio_data.channels,
        sample_rate: audio_data.sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };

    let mut writer = WavWriter::create(path, spec)?;
    for sample in &audio_data.samples {
        writer
            .write_sample(*sample)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
    }
    writer.finalize().map_err(|e| anyhow::anyhow!("{}", e))?;
    Ok(())
}
