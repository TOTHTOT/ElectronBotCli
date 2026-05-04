pub mod asr;
mod tts;

use crate::media::audio::asr::{Recognition, RecognitionConfig};
use anyhow::{bail, Result};
use boteyes::Mood;
use cpal::traits::{DeviceTrait, HostTrait};
use droid_chatter::{setup_sounds, AudioData, DroidChatter, Mood as DroidMood};
use std::num::NonZero;
use std::path::Path;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

pub const VAD_WINDOW_SIZE: i32 = 512;
#[allow(dead_code)]
pub const CHUNK_SIZE: usize = 1600; // 100ms at 16kHz
#[allow(dead_code)]
pub const SAMPLE_RATE: u32 = 16000;
#[allow(dead_code)]
pub struct VoiceManager {
    pub rx: mpsc::Receiver<String>,
    _recognition: Recognition,
    volume: Arc<AtomicI32>,
    speaker_name: String,
    microphone_name: String,
}

#[allow(dead_code)]
impl VoiceManager {
    pub fn new(
        sense_voice_model_path: impl AsRef<Path>,
        silero_vad_model_path: impl AsRef<Path>,
        tokens_path: impl AsRef<Path>,
        speaker_name: &str,
        microphone_name: &str,
    ) -> Result<Self> {
        let volume = Arc::new(AtomicI32::new(0));
        let sense_voice_model_path = sense_voice_model_path.as_ref().into();
        let silero_vad_model_path = silero_vad_model_path.as_ref().into();
        let tokens_path = tokens_path.as_ref().into();

        // 初始化 麦克风以及语音识别
        let microphone_name_str = microphone_name.to_string();
        let (text_tx, text_rx) = mpsc::channel::<String>();
        let config = RecognitionConfig::new(
            sense_voice_model_path,
            silero_vad_model_path,
            tokens_path,
            text_tx,
            microphone_name_str,
            volume.clone(),
        );
        let mut recognition = Recognition::new(config);
        if let Err(e) = recognition.recognition_start() {
            log::error!("recognition_start failed: {e:?}");
        }

        Ok(Self {
            _recognition: recognition,
            speaker_name: speaker_name.to_string(),
            microphone_name: microphone_name.to_string(),
            volume,
            rx: text_rx,
        })
    }

    pub fn volume(&self) -> i32 {
        self.volume.load(Ordering::Relaxed)
    }
}

/// 使用 droid-chatter 库播放 BD1 机器人声音
pub fn play_bd1_sound(mood: Mood, phrase: &str) -> Result<()> {
    let temp_dir = std::env::current_dir()?;
    let sounds_dir = temp_dir.join("sounds");
    // 首次调用时下载声音文件
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        if let Err(e) = setup_sounds(&sounds_dir) {
            log::error!("Failed to setup droid sounds: {e}, phrase: {phrase}");
        }
    });

    let chatter = match DroidChatter::new(&sounds_dir) {
        Ok(c) => c,
        Err(e) => {
            bail!("Failed to create DroidChatter: {e}, phrase: {phrase}");
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
    let audio_data = match chatter.bd1_audio(phrase, droid_mood) {
        Ok(a) => a,
        Err(e) => {
            bail!("Failed to get BD1 audio: {e}");
        }
    };

    // 根据平台选择设备名称
    #[cfg(target_os = "linux")]
    let device_name = "sysdefault:CARD=CODEC";
    #[cfg(target_os = "macos")]
    let device_name = "BuiltInSpeakerDevice";

    // 使用 rodio 播放音频
    if let Err(e) = play_audio(&audio_data, device_name) {
        bail!("Failed to play audio: {}", e);
    }
    log::info!("Audio played, phrase: {}, mood: {:?}", phrase, mood);
    Ok(())
}
#[allow(dead_code)]
fn write_audio_to_wav(audio_data: &AudioData, path: &Path) -> Result<()> {
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

/// 播放原始音频, 不同平台使用的设备名称不一样, 阻塞直到播放完成
///
/// # Arguments
///
/// * `audio_data`: 音频数据
/// * `device_name`: 播放的设备名称
///
/// returns: Result<(), Error>
///
/// # Examples
///
/// ```
/// #[cfg(target_os = "linux")]
/// play_audio(&audio_data, "sysdefault:CARD=CODEC")?;
/// #[cfg(target_os = "macos")]
/// play_audio(&audio_data, "BuiltInSpeakerDevice")?;
/// ```
pub fn play_audio(audio_data: &AudioData, device_name: &str) -> Result<()> {
    let host = cpal::default_host();
    let mut devices = host.output_devices()?;

    let device = devices
        .find(|d| d.id().map(|n| n.1.contains(device_name)).unwrap_or(false))
        .ok_or_else(|| anyhow::anyhow!("Find: {} failed.", device_name))?;

    // rodio 0.22 的 Sample = f32，必须转换
    let samples_f32: Vec<f32> = audio_data
        .samples
        .iter()
        .map(|&s| s as f32 / i16::MAX as f32)
        .collect();

    // 创建 SamplesBuffer
    let buffer = rodio::buffer::SamplesBuffer::new(
        NonZero::new(audio_data.channels).unwrap(),
        NonZero::new(audio_data.sample_rate).unwrap(),
        samples_f32,
    );

    let sink = rodio::DeviceSinkBuilder::from_device(device)
        .map_err(|e| anyhow::anyhow!("Failed to create sink: {}", e))?
        .open_stream()?;
    let mixer = sink.mixer();
    mixer.add(buffer);

    // 计算播放时长并等待, 使用Buffer机制的没有内部阻塞, 只能手动计算了
    let total_samples = audio_data.samples.len() as f64 / audio_data.channels as f64;
    let duration_secs = total_samples / audio_data.sample_rate as f64;
    thread::sleep(Duration::from_secs_f64(duration_secs + 0.5));

    Ok(())
}
