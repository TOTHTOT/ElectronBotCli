#![allow(dead_code)]

use crate::media::voice::VAD_WINDOW_SIZE;
use cpal::traits::DeviceTrait;
use cpal::{Device, SampleRate, Stream};
use sherpa_onnx::{
    OfflineModelConfig, OfflineRecognizer, OfflineRecognizerConfig, OfflineSenseVoiceModelConfig,
    VadModelConfig, VoiceActivityDetector,
};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender};
use std::sync::{mpsc, Arc};
use std::time::Duration;

const SAMPLE_RATE: usize = 16000;
const PRE_ROLL_MS: usize = 500;
const PRE_ROLL_SAMPLES: usize = SAMPLE_RATE / 1000 * PRE_ROLL_MS;
const SILENCE_THRESHOLD: usize = 120;
const MIN_AUDIO_LEN: usize = SAMPLE_RATE / 2;

/// Initialize SenseVoice recognizer using sherpa-onnx
fn init_sense_voice(model_path: &Path, tokens_path: &Path) -> anyhow::Result<OfflineRecognizer> {
    let config = OfflineRecognizerConfig {
        model_config: OfflineModelConfig {
            sense_voice: OfflineSenseVoiceModelConfig {
                model: Some(model_path.to_string_lossy().to_string()),
                language: Some("auto".to_string()),
                use_itn: true,
            },
            tokens: Some(tokens_path.to_string_lossy().to_string()),
            ..Default::default()
        },
        ..Default::default()
    };

    OfflineRecognizer::create(&config)
        .ok_or_else(|| anyhow::anyhow!("Failed to create SenseVoice recognizer"))
}

/// Initialize Silero VAD
fn init_silero_vad(model_path: &Path) -> anyhow::Result<VoiceActivityDetector> {
    let vad_config = VadModelConfig {
        silero_vad: sherpa_onnx::SileroVadModelConfig {
            model: Some(model_path.to_string_lossy().to_string()),
            threshold: 0.5,
            min_silence_duration: 0.3,
            min_speech_duration: 0.3,
            window_size: VAD_WINDOW_SIZE,
            max_speech_duration: 30.0,
        },
        ..Default::default()
    };

    VoiceActivityDetector::create(&vad_config, 600.0)
        .ok_or_else(|| anyhow::anyhow!("Failed to create VAD"))
}

/// Speech recognition main loop: VAD detection + SenseVoice recognition
///
/// 用 `audio_rx.recv_timeout(50ms)` 替代阻塞 `recv`, 每轮检查
/// `running` 标志: 一旦为 false, 立即返回. 这是 `rebuild_voice`
/// 软替换旧实例的退出通道, 不依赖 cpal backend 及时停回调.
fn recognition_loop(
    recognizer: &mut OfflineRecognizer,
    vad: &mut VoiceActivityDetector,
    audio_rx: Receiver<Vec<f32>>,
    result_tx: mpsc::Sender<String>,
    running: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let mut buffer: Vec<f32> = Vec::new();
    let mut speaking = false;
    let mut silence_count = 0;
    let mut pre_roll: VecDeque<f32> = VecDeque::with_capacity(PRE_ROLL_SAMPLES);

    loop {
        let samples = match audio_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(s) => s,
            Err(RecvTimeoutError::Timeout) => {
                // 50ms 没数据, 检查取消信号. cpal Stream 已经 Drop
                // 时 audio_rx 也会 Disconnected, 走下面分支.
                if !running.load(Ordering::Relaxed) {
                    log::info!("ASR receive exit flag");
                    return Ok(());
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => {
                // audio_tx 所有克隆都已 drop (旧 cpal Stream 被替换).
                return Ok(());
            }
        };

        // Sliding window: keep latest 500ms
        while pre_roll.len() >= PRE_ROLL_SAMPLES {
            pre_roll.pop_front();
        }
        pre_roll.extend(&samples);

        // VAD detection: use pre_roll (already includes current samples)
        let is_speech = if pre_roll.len() >= VAD_WINDOW_SIZE as usize * 2 {
            let all_samples: Vec<f32> = pre_roll.iter().copied().collect();
            // Ensure we have enough samples for VAD
            if all_samples.len() >= 512 {
                vad.accept_waveform(&all_samples[..all_samples.len().min(512)]);
            }
            vad.detected()
        } else {
            false
        };

        if is_speech {
            if !speaking {
                log::info!(">>> Speech start");
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
                    "<<< Speech end, {:?}s",
                    buffer.len() as f32 / SAMPLE_RATE as f32
                );

                if buffer.len() > MIN_AUDIO_LEN {
                    // Use sherpa-onnx recognizer - create stream and feed audio
                    let stream = recognizer.create_stream();
                    stream.accept_waveform(SAMPLE_RATE as i32, &buffer);
                    recognizer.decode(&stream);

                    if let Some(result) = stream.get_result() {
                        let text = result.text.trim().to_string();
                        if !text.is_empty() {
                            log::info!("ASR: 【{}】", text);
                            let _ = result_tx.send(text);
                        }
                    }
                }

                buffer.clear();
                speaking = false;
                silence_count = 0;
                vad.clear();
            }
        }
    }
}

/// 把 cpal f32 峰值样本 (0.0..=1.0) 映射成 0..=100 的归一化音量.
///
/// 用 dB 对数刻度: 0 dB (peak=1.0) -> 100, -40 dB (peak=0.01) -> 0.
/// 这样小声说话 (peak 0.05–0.3) 能稳定显示在 30–70, 不再退化成 1–3 格
/// 音量条. -40 dB 是常见麦克风本底噪声量级, 低于此值视为静音.
fn peak_to_volume(peak: f32) -> i32 {
    if peak <= 0.0 {
        return 0;
    }
    let db = 20.0 * peak.log10();
    (((db + 40.0) * (100.0 / 40.0)) as i32).clamp(0, 100)
}

/// 计算一帧音频的峰值音量并推进音量条 (attack/decay), 然后 downmix
/// 到单声道推给识别线程.
///
/// 音量条更新: 新峰值立即拉升, 否则按 0.95/帧 指数衰减
/// (半衰期约 0.4s). 多声道按左右声道算术均值降混; 单声道直通.
/// 函数不获取 `volume` / `audio_tx` 的所有权, 由调用方持有 (通常是
/// cpal 输入流闭包, 闭包负责把变量 move 进来).
fn process_audio_chunk(
    data: &[f32],
    channels: usize,
    volume: &Arc<AtomicI32>,
    audio_tx: &SyncSender<Vec<f32>>,
) {
    let peak = data.iter().fold(0.0f32, |acc, &s| acc.max(s.abs()));
    let peak_value = peak_to_volume(peak);
    let current = volume.load(Ordering::Relaxed);
    // 实时音量大小
    let new_value = if peak_value > current {
        peak_value
    } else if current > 0 {
        ((current as f32) * 0.95) as i32
    } else {
        0
    };
    volume.store(new_value, Ordering::Relaxed);

    // 音频数据
    let mono: Vec<f32> = if channels == 2 {
        data.chunks(2).map(|c| (c[0] + c[1]) / 2.0).collect()
    } else {
        data.to_vec()
    };
    let _ = audio_tx.send(mono);
}

/// Build audio input stream for ASR
pub fn build_asr_stream(
    device: &Device,
    volume: Arc<AtomicI32>,
    audio_tx: SyncSender<Vec<f32>>,
) -> anyhow::Result<Stream> {
    let config = device.default_input_config()?;
    log::info!("Audio device: {:?}", config);

    let stream_config = cpal::StreamConfig {
        channels: config.channels(),
        sample_rate: SAMPLE_RATE as SampleRate,
        buffer_size: cpal::BufferSize::Fixed(512),
    };

    let channels = config.channels() as usize;
    let volume_clone = volume.clone();

    // 根据设置的stream_config申请音频流
    Ok(device.build_input_stream(
        &stream_config,
        move |data: &[f32], _: &_| {
            process_audio_chunk(data, channels, &volume_clone, &audio_tx);
        },
        |e| log::error!("Audio stream error: {e}"),
        None,
    )?)
}

/// Speech recognition thread entry
pub fn recognition_thread(
    sense_voice_model_path: PathBuf,
    silero_vad_model_path: PathBuf,
    tokens_path: PathBuf,
    audio_rx: Receiver<Vec<f32>>,
    result_tx: mpsc::Sender<String>,
    running: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let mut recognizer = init_sense_voice(&sense_voice_model_path, &tokens_path)?;
    let mut vad = init_silero_vad(&silero_vad_model_path)?;
    recognition_loop(&mut recognizer, &mut vad, audio_rx, result_tx, running)
}

/// Use `cargo test test_recognition_with_audio_file -- --ignored --nocapture` to test
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
        assert!(
            samples.iter().all(|&s| (-1.0..=1.0).contains(&s)),
            "sample out of range"
        );
    }

    #[test]
    fn peak_to_volume_mapping() {
        // 静音 -> 0
        assert_eq!(peak_to_volume(0.0), 0);
        // 麦克风底噪附近 (-46 dB) -> 0
        assert_eq!(peak_to_volume(0.005), 0);
        // 小声说话 (peak ~ 0.05, -26 dB) 应该在 30-40 之间
        let small = peak_to_volume(0.05);
        assert!(
            (30..=40).contains(&small),
            "peak=0.05 expected ~35, got {small}"
        );
        // 中等说话 (peak ~ 0.1, -20 dB) 在 50-55
        let mid = peak_to_volume(0.1);
        assert!((50..=55).contains(&mid), "peak=0.1 expected ~50, got {mid}");
        // 大声 (peak ~ 0.5, -6 dB) 在 85 附近
        let loud = peak_to_volume(0.5);
        assert!(
            (80..=90).contains(&loud),
            "peak=0.5 expected ~85, got {loud}"
        );
        // 满刻度 -> 100
        assert_eq!(peak_to_volume(1.0), 100);
        // 超过 1.0 (理论上不会, 但保险) -> clamp 到 100
        assert_eq!(peak_to_volume(2.0), 100);
    }

    #[test]
    #[ignore]
    fn test_recognition_with_audio_file() {
        let samples = load_wav_samples(Path::new("assets/audio/asr_example_zh.wav"))
            .expect("failed to load wav");

        let model_path = ModelManager::global()
            .get("sense_voice")
            .expect("sense_voice model not found");
        let vad_path = ModelManager::global()
            .get("silero_vad")
            .expect("silero_vad model not found");
        let tokens_path = ModelManager::global()
            .get("sense_voice_tokens")
            .expect("sense_voice_tokens not found");

        let mut recognizer =
            init_sense_voice(&model_path, &tokens_path).expect("Failed to create recognizer");
        let mut vad = init_silero_vad(&vad_path).expect("Failed to create VAD");

        let (audio_tx, audio_rx) = mpsc::sync_channel::<Vec<f32>>(4);
        let (result_tx, result_rx) = mpsc::channel::<String>();

        // Feed audio in a separate thread since audio_tx doesn't implement Send
        let handle = std::thread::spawn(move || {
            let chunk_size = 1600;
            for chunk in samples.chunks(chunk_size) {
                audio_tx.send(chunk.to_vec()).expect("send failed");
            }

            // Send silence frames to give VAD time to detect end of speech
            for _ in 0..150 {
                audio_tx
                    .send(vec![0.0f32; chunk_size])
                    .expect("send failed");
            }
            drop(audio_tx);
        });

        // Run recognition on main thread since recognizer/vad are not Send
        let running = Arc::new(AtomicBool::new(true));
        let _ = recognition_loop(&mut recognizer, &mut vad, audio_rx, result_tx, running);

        handle.join().expect("thread panicked");

        let results: Vec<String> = result_rx.iter().collect();
        assert!(!results.is_empty(), "no recognition results");
        println!("Recognition results: {:?}", results);
    }
}
