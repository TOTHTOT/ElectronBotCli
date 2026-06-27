pub mod asr;
pub mod tts;

use crate::media::voice::asr::{build_asr_stream, recognition_thread};
use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Stream};
use std::path::Path;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;

use self::tts::{TtsHandler, TtsPlayer};

pub const VAD_WINDOW_SIZE: i32 = 512;
#[allow(dead_code)]
pub const CHUNK_SIZE: usize = 1600; // 100ms at 16kHz
#[allow(dead_code)]
pub const SAMPLE_RATE: u32 = 16000;
#[allow(dead_code)]
pub struct VoiceManager {
    _stream: Option<Stream>,
    _rx: mpsc::Receiver<String>,
    volume: Arc<AtomicI32>,
    tts_handler: TtsHandler,
    tts_player: Option<TtsPlayer>,
}

// Safety: VoiceManager is only accessed from the main thread and the ASR thread.
// The Receiver is only used in the ASR thread, and the TTS methods are called
// from the main thread, so it's safe to implement Send + Sync.
#[allow(dead_code)]
unsafe impl Send for VoiceManager {}
#[allow(dead_code)]
unsafe impl Sync for VoiceManager {}

#[allow(dead_code)]
impl VoiceManager {
    /// 创建voice模块, 通过静音检测截取有效实时音频数据,
    /// 完成后发送到解析线程
    pub fn new(
        sense_voice_model_path: impl AsRef<Path>,
        silero_vad_model_path: impl AsRef<Path>,
        tokens_path: impl AsRef<Path>,
        speech_name: &str,
        tts_model_path: impl AsRef<Path>,
        tts_tokens_path: impl AsRef<Path>,
        tts_lexicon_path: impl AsRef<Path>,
        output_device_name: &str,
    ) -> Result<Self> {
        // 初始化 TTS
        let tts_handler = TtsHandler::new(&tts_model_path, &tts_tokens_path, &tts_lexicon_path)?;
        let tts_player = Some(TtsPlayer::new(output_device_name)?);

        let volume = Arc::new(AtomicI32::new(0)); // 实时音量
        let (audio_tx, audio_rx) = mpsc::sync_channel::<Vec<f32>>(4); // 原始音频数据传输通道

        // 查找输入麦克风, 当设备不存在时也会继续执行, 只是不会运行到 asr 相关功能
        let stream = match find_input_device(speech_name) {
            Ok(device) => {
                let stream = build_asr_stream(&device, volume.clone(), audio_tx)?;
                stream.play()?;
                Some(stream)
            }
            Err(e) => {
                log::warn!("Cannot find input device: {}", e);
                None
            }
        };

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
            _rx: text_rx,
            volume,
            tts_handler,
            tts_player,
        })
    }

    /// 获取实时音量
    pub fn volume(&self) -> i32 {
        self.volume.load(Ordering::Relaxed)
    }

    /// 获取 TTS handler
    pub fn tts_handler(&self) -> &TtsHandler {
        &self.tts_handler
    }

    /// 使用 TTS 播放文本
    ///
    /// # Arguments
    ///
    /// * `text`:
    /// * `speed`:
    /// * `on_complete`: 播放完成时的回调 (播放结果, 是否出错)
    ///
    /// returns: Result<(), Error>
    pub fn speak(
        &self,
        text: &str,
        speed: f32,
        on_complete: Option<Box<dyn FnOnce(Result<()>) + Send>>,
    ) -> Result<()> {
        let start = std::time::Instant::now();
        let audio = self.tts_handler.synthesize(text, speed)?;
        log::info!("synthesized audio took: {:?}", start.elapsed());
        if let Some(player) = &self.tts_player {
            player.play(&audio)?;
            if let Some(callback) = on_complete {
                callback(Ok(()));
            }
        }
        Ok(())
    }

    /// 流式播放 TTS - 每个音频片段生成后立即播放
    ///
    /// # Arguments
    /// * `text` - 要播放的文本
    /// * `speed` - 播放速度
    /// * `on_complete` - 播放完成时的回调 (播放结果, 是否出错)
    pub fn speak_streaming(
        &self,
        text: &str,
        speed: f32,
        on_complete: Option<Box<dyn FnOnce(Result<()>) + Send>>,
    ) -> Result<()> {
        let player = self
            .tts_player
            .as_ref()
            .ok_or_else(|| anyhow!("TTS player not available"))?;
        let sample_rate = self.tts_handler.sample_rate();
        let stream_handle = player.start_streaming(sample_rate)?;

        let handle = Arc::new(stream_handle);

        // Spawn a new thread for synthesis to run in parallel with playback
        let handle_clone = handle.clone();
        let text = text.to_string();
        let tts_handler = self.tts_handler.clone();
        let on_complete = std::sync::Mutex::new(on_complete);

        thread::spawn(move || {
            if let Err(e) = tts_handler.synthesize_streaming(&text, speed, {
                let handle = handle_clone.clone();
                move |chunk, progress| {
                    handle.write_chunk(chunk, progress);
                }
            }) {
                log::error!("TTS synthesis error: {e:?}");
            }
            handle_clone.mark_synthesis_done();
            log::info!("TTS synthesis done");
        });

        // Wait for playback to finish
        while !handle.is_done() {
            thread::sleep(std::time::Duration::from_millis(50));
        }

        log::info!("TTS streaming playback done");

        // 调用完成回调
        if let Some(callback) = on_complete.into_inner().unwrap_or(None) {
            callback(Ok(()));
        }

        Ok(())
    }

    /// 检查 TTS 是否可用
    pub fn is_tts_available(&self) -> bool {
        self.tts_player.is_some()
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
        "input audio device: {:?}",
        devices.iter().map(|(name, _)| name).collect::<Vec<_>>()
    );
    // 名称为空时回退到默认输入设备
    if speech_name.is_empty() {
        return host
            .default_input_device()
            .ok_or_else(|| anyhow!("No default audio input device found"));
    }
    let device = devices
        .iter()
        .find(|(name, _)| name == speech_name)
        .map(|(_, d)| d.clone())
        .ok_or_else(|| anyhow!("No audio input device found: {}", speech_name))?;

    // 打印设备配置信息
    if let Ok(config) = device.default_input_config() {
        log::info!("Selected audio device config: {config:?} ");
    }

    Ok(device)
}

/// 设备信息 - 用于在设置页面中显示并区分同名设备
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// 实际设备名称
    pub name: String,
    /// 列表中显示的字符串, 包含通道数 / 采样率等额外信息
    pub display: String,
}

impl DeviceInfo {
    fn new(
        name: String,
        channels: Option<u16>,
        sample_rate: Option<u32>,
        _index: usize,
        driver: Option<String>,
    ) -> Self {
        let mut parts: Vec<String> = Vec::new();
        if let Some(driver) = driver {
            parts.push(format!("{} ", driver));
        }
        if let Some(ch) = channels {
            parts.push(format!("{}ch", ch));
        }
        if let Some(sr) = sample_rate {
            parts.push(format!("{}Hz", sr));
        }
        let display = if parts.is_empty() {
            name.clone()
        } else {
            format!("{} ({})", name, parts.join(", "))
        };
        Self { name, display }
    }
}

/// 枚举系统所有输入设备, 包含通道数和采样率等额外信息
pub fn list_input_devices() -> Vec<DeviceInfo> {
    let host = cpal::default_host();
    host.input_devices()
        .map(|it| {
            it.enumerate()
                .filter_map(|(i, d)| {
                    let desc = d.description().ok()?;
                    let name = desc.name().to_string();
                    let driver = desc.driver().map(|s| s.to_string());
                    let (channels, sample_rate) = d
                        .default_input_config()
                        .map(|c| (Some(c.channels()), Some(c.sample_rate())))
                        .unwrap_or((None, None));
                    Some(DeviceInfo::new(name, channels, sample_rate, i, driver))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 枚举系统所有输出设备, 包含通道数和采样率等额外信息
pub fn list_output_devices() -> Vec<DeviceInfo> {
    let host = cpal::default_host();
    host.output_devices()
        .map(|it| {
            it.enumerate()
                .filter_map(|(i, d)| {
                    let desc = d.description().ok()?;
                    let name = desc.name().to_string();
                    let driver = desc.driver().map(|s| s.to_string());
                    let (channels, sample_rate) = d
                        .default_output_config()
                        .map(|c| (Some(c.channels()), Some(c.sample_rate())))
                        .unwrap_or((None, None));
                    Some(DeviceInfo::new(name, channels, sample_rate, i, driver))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 按名称查找输出设备，名称为空或找不到时回退到默认输出设备
pub fn find_output_device(name: &str) -> Option<Device> {
    let host = cpal::default_host();
    if !name.is_empty() {
        if let Ok(devices) = host.output_devices() {
            for d in devices {
                if let Ok(desc) = d.description() {
                    if desc.name() == name {
                        return Some(d);
                    }
                }
            }
        }
        log::warn!(
            "Output device '{}' not found, falling back to default",
            name
        );
    }
    host.default_output_device()
}

/// Play a simple beep sound (for notifications)
pub fn play_beep(
    count: u32,
    frequency: f32,
    duration_ms: u32,
    interval_ms: u32,
    output_device_name: &str,
) {
    let device = match find_output_device(output_device_name) {
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
        write_audio_callback(samples),
        |err| log::error!("Beep stream error: {}", err),
        None,
    )?;

    stream.play()?;
    thread::sleep(std::time::Duration::from_millis(duration_ms as u64 + 50));

    Ok(())
}

/// Creates a callback closure for writing audio samples to an output stream
pub fn write_audio_callback(
    samples: Vec<f32>,
) -> impl FnMut(&mut [f32], &cpal::OutputCallbackInfo) + Send + 'static {
    move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
        for (i, sample) in data.iter_mut().enumerate() {
            *sample = samples.get(i).copied().unwrap_or(0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::media::voice::list_output_devices;

    #[test]
    fn show_all_output_devices() {
        let devices = list_output_devices();
        println!("{:?}", devices);
    }
}
