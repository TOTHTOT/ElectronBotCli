use crate::media::audio::VAD_WINDOW_SIZE;
use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{Device, SampleRate, Stream};
use sherpa_rs::sense_voice::{SenseVoiceConfig, SenseVoiceRecognizer};
use sherpa_rs::silero_vad::{SileroVad, SileroVadConfig};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;

const SAMPLE_RATE: usize = 16000;
const PRE_ROLL_MS: usize = 500;
const PRE_ROLL_SAMPLES: usize = SAMPLE_RATE / 1000 * PRE_ROLL_MS;
const SILENCE_THRESHOLD: usize = 120;
const MIN_AUDIO_LEN: usize = SAMPLE_RATE / 2;

pub struct Recognition {
    config: RecognitionConfig,
    handle: Option<JoinHandle<Result<(), anyhow::Error>>>,
    stream: Option<Stream>,
}
impl Recognition {
    pub fn new(config: RecognitionConfig) -> Recognition {
        Self {
            config,
            handle: None,
            stream: None,
        }
    }
    pub fn recognition_start(&mut self) -> anyhow::Result<()> {
        let (audio_tx, audio_rx) = mpsc::sync_channel::<Vec<f32>>(4);
        let input_stream = self.config.build_input_stream(audio_tx)?;

        let sense_path = self.config.sense_voice_model_path.clone();
        let silero_path = self.config.silero_vad_model_path.clone();
        let tokens_path = self.config.tokens_path.clone();
        let tx_clone = self.config.result_tx.clone();
        let handle = std::thread::spawn(move || {
            let mut recognizer = Recognition::init_sense_voice(&sense_path, &tokens_path)?;
            let mut vad = Recognition::init_silero_vad(&silero_path)?;
            Recognition::recognition_loop(&mut recognizer, &mut vad, audio_rx, tx_clone)
        });

        // 持久化, 免得直接drop了
        self.handle = Some(handle);
        self.stream = Some(input_stream);
        Ok(())
    }

    /// 初始化 SenseVoice 识别器
    fn init_sense_voice(
        model_path: &Path,
        tokens_path: &Path,
    ) -> anyhow::Result<SenseVoiceRecognizer> {
        let config = SenseVoiceConfig {
            model: model_path.to_string_lossy().into(),
            tokens: tokens_path.to_string_lossy().into(),
            #[cfg(target_os = "windows")]
            provider: Some("cpu".into()),
            ..Default::default()
        };
        SenseVoiceRecognizer::new(config).map_err(|e| anyhow::anyhow!("SenseVoice failed: {e:?}"))
    }

    /// 初始化 Silero VAD
    fn init_silero_vad(model_path: &Path) -> anyhow::Result<SileroVad> {
        let vad_config = SileroVadConfig {
            model: model_path.to_string_lossy().into(),
            window_size: VAD_WINDOW_SIZE,
            ..Default::default()
        };
        SileroVad::new(vad_config, 600.0).map_err(|e| anyhow::anyhow!("SileroVad failed: {e}"))
    }

    /// 语音识别主循环：VAD 检测 + SenseVoice 识别
    fn recognition_loop(
        recognizer: &mut SenseVoiceRecognizer,
        vad: &mut SileroVad,
        audio_rx: Receiver<Vec<f32>>,
        result_tx: mpsc::Sender<String>,
    ) -> anyhow::Result<()> {
        let mut buffer: Vec<f32> = Vec::new();
        let mut speaking = false;
        let mut silence_count = 0;
        let mut pre_roll: VecDeque<f32> = VecDeque::with_capacity(PRE_ROLL_SAMPLES);

        for samples in audio_rx {
            // 滑动窗口：保留最近 500ms
            while pre_roll.len() >= PRE_ROLL_SAMPLES {
                pre_roll.pop_front();
            }
            pre_roll.extend(&samples);

            // VAD 检测：使用 pre_roll（已包含当前 samples）
            vad.accept_waveform(pre_roll.iter().copied().collect());

            if vad.is_speech() {
                if !speaking {
                    log::info!(">>> 语音开始");
                    speaking = true;
                    buffer.extend(&pre_roll);
                }
                buffer.extend(&samples);
                silence_count = 0;
            } else if speaking {
                silence_count += 1;
                buffer.extend(&samples);

                if silence_count > SILENCE_THRESHOLD {
                    log::info!(
                        "<<< 语音结束，{:?}s",
                        buffer.len() as f32 / SAMPLE_RATE as f32
                    );

                    if buffer.len() > MIN_AUDIO_LEN {
                        let result = recognizer.transcribe(SAMPLE_RATE as u32, &buffer);
                        let text = result.text.trim().to_string();
                        if !text.is_empty() {
                            log::info!("ASR: 【{}】", text);
                            let _ = result_tx.send(text);
                        }
                    }

                    buffer.clear();
                    speaking = false;
                    silence_count = 0;
                    vad.clear();
                }
            }
        }

        Ok(())
    }
}

impl Drop for Recognition {
    fn drop(&mut self) {
        self.stream = None;
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
            log::info!("Recognition stopped");
        }
    }
}

pub struct RecognitionConfig {
    sense_voice_model_path: PathBuf,
    silero_vad_model_path: PathBuf,
    tokens_path: PathBuf,
    result_tx: mpsc::Sender<String>,
    microphone_name: String,
    volume: Arc<AtomicI32>,
}
impl RecognitionConfig {
    pub fn new(
        sense_voice_model_path: PathBuf,
        silero_vad_model_path: PathBuf,
        tokens_path: PathBuf,
        result_tx: mpsc::Sender<String>,
        microphone_name: String,
        volume: Arc<AtomicI32>,
    ) -> Self {
        Self {
            sense_voice_model_path,
            silero_vad_model_path,
            tokens_path,
            result_tx,
            microphone_name,
            volume,
        }
    }

    fn find_input_device(&self) -> anyhow::Result<Device> {
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
            .find(|(name, _)| name.contains(&self.microphone_name))
            .map(|(_, d)| d.clone())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "No audio input device found containing: {}",
                    self.microphone_name
                )
            })?;
        log::info!("use microphone: {}", device.id()?.1);
        if let Ok(config) = device.default_input_config() {
            log::info!("Selected audio device config: {config:?} ");
        }

        Ok(device)
    }

    /// 构建音频输入流
    fn build_input_stream(&self, audio_tx: SyncSender<Vec<f32>>) -> anyhow::Result<Stream> {
        let device = self.find_input_device()?;

        let config = device.default_input_config()?;
        log::info!("Audio device: {:?}", config);

        let stream_config = cpal::StreamConfig {
            channels: config.channels(),
            sample_rate: SAMPLE_RATE as SampleRate,
            buffer_size: cpal::BufferSize::Fixed(512),
        };
        log::info!("Stream config: {}", stream_config.sample_rate);

        let channels = config.channels() as usize;
        let volume_clone = self.volume.clone();

        Ok(device.build_input_stream(
            &stream_config,
            move |data: &[f32], _: &_| {
                let sum: f32 = data.iter().map(|&s| s * s).sum();
                let rms = (sum / data.len() as f32).sqrt();
                volume_clone.store((rms * 100.0).min(100.0) as i32, Ordering::Relaxed);

                let mono: Vec<f32> = if channels == 2 {
                    data.chunks(2).map(|c| (c[0] + c[1]) / 2.0).collect()
                } else {
                    data.to_vec()
                };
                let _ = audio_tx.send(mono);
            },
            |e| log::error!("Audio stream error: {e}"),
            None,
        )?)
    }
}
/// 语音识别线程入口

/// 使用`cargo test test_recognition_with_audio_file -- --ignored --nocapture` 测试模块
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_manager::ModelManager;
    use std::fs::File;
    use std::io::Read;
    use std::path::Path;

    fn load_wav_samples(path: &Path) -> anyhow::Result<Vec<f32>> {
        let mut file = File::open(path)?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;
        let pcm_data = &buffer[44..];
        let samples: Vec<f32> = pcm_data
            .chunks_exact(2)
            .map(|chunk| {
                let s = i16::from_le_bytes([chunk[0], chunk[1]]);
                s as f32 / 32768.0
            })
            .collect();
        Ok(samples)
    }

    #[test]
    fn test_load_wav_samples() {
        let samples = load_wav_samples(Path::new("assets/audio/asr_example_zh.wav"))
            .expect("failed to load wav");
        assert!(!samples.is_empty());
        assert!(samples.len() >= 16000);
        for &s in &samples {
            assert!(s >= -1.0 && s <= 1.0, "sample out of range: {}", s);
        }
    }

    #[test]
    #[ignore]
    fn test_recognition_with_audio_file() {
        let samples = load_wav_samples(Path::new("assets/audio/asr_example_zh.wav"))
            .expect("failed to load wav");

        let model_path = ModelManager::global().get("sense_voice").unwrap();
        let vad_path = ModelManager::global().get("silero_vad").unwrap();
        let tokens_path = ModelManager::global().get("sense_voice_tokens").unwrap();

        let mut recognizer = Recognition::init_sense_voice(&model_path, &tokens_path).unwrap();
        let mut vad = Recognition::init_silero_vad(&vad_path).unwrap();

        let (audio_tx, audio_rx) = mpsc::sync_channel::<Vec<f32>>(4);
        let (result_tx, result_rx) = mpsc::channel::<String>();

        let handle = std::thread::spawn(move || {
            Recognition::recognition_loop(&mut recognizer, &mut vad, audio_rx, result_tx)
        });

        let chunk_size = 1600;
        for chunk in samples.chunks(chunk_size) {
            audio_tx.send(chunk.to_vec()).expect("send failed");
        }

        // 发送静默帧，让 VAD 有时间检测到语音结束
        let silence_chunk = vec![0.0f32; chunk_size];
        for _ in 0..150 {
            audio_tx.send(silence_chunk.clone()).expect("send failed");
        }
        drop(audio_tx);

        let results: Vec<String> = result_rx.iter().collect();
        handle
            .join()
            .expect("thread panicked")
            .expect("loop failed");

        assert!(!results.is_empty(), "no recognition results");
        println!("Recognition results: {:?}", results);
    }
}
