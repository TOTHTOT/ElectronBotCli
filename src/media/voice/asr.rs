use crate::media::voice::VAD_WINDOW_SIZE;
use cpal::traits::DeviceTrait;
use cpal::{Device, SampleRate, Stream};
use sherpa_rs::sense_voice::{SenseVoiceConfig, SenseVoiceRecognizer};
use sherpa_rs::silero_vad::{SileroVad, SileroVadConfig};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::{mpsc, Arc};

const SAMPLE_RATE: usize = 16000;
const PRE_ROLL_MS: usize = 500;
const PRE_ROLL_SAMPLES: usize = SAMPLE_RATE / 1000 * PRE_ROLL_MS;
const SILENCE_THRESHOLD: usize = 120;
const MIN_AUDIO_LEN: usize = SAMPLE_RATE / 2;

/// 构建音频输入流
pub fn build_audio_stream(
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
    log::info!("Stream config: {}", stream_config.sample_rate);

    let channels = config.channels() as usize;
    let volume_clone = volume.clone();

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

/// 语音识别主循环：VAD 检测 + SenseVoice 识别
pub fn recognition_thread(
    sense_voice_model_path: PathBuf,
    silero_vad_model_path: PathBuf,
    tokens_path: PathBuf,
    audio_rx: Receiver<Vec<f32>>,
    result_tx: mpsc::Sender<String>,
) -> anyhow::Result<()> {
    let config = SenseVoiceConfig {
        model: sense_voice_model_path.to_string_lossy().into(),
        tokens: tokens_path.to_string_lossy().into(),
        #[cfg(target_os = "windows")]
        provider: Some("cpu".into()),
        ..Default::default()
    };
    let mut recognizer = SenseVoiceRecognizer::new(config)
        .map_err(|e| anyhow::anyhow!("SenseVoice failed: {e:?}"))?;

    let vad_config = SileroVadConfig {
        model: silero_vad_model_path.to_string_lossy().into(),
        window_size: VAD_WINDOW_SIZE,
        ..Default::default()
    };
    let mut vad =
        SileroVad::new(vad_config, 600.0).map_err(|e| anyhow::anyhow!("SileroVad failed: {e}"))?;

    let mut buffer = Vec::new();
    let mut speaking = false;
    let mut silence_count = 0;
    let mut pre_roll = VecDeque::with_capacity(PRE_ROLL_SAMPLES);

    for samples in audio_rx {
        // 滑动窗口：保留最近 500ms
        for &s in &samples {
            if pre_roll.len() >= PRE_ROLL_SAMPLES {
                pre_roll.pop_front();
            }
            pre_roll.push_back(s);
        }

        // VAD 完整上下文
        let mut all: Vec<f32> = pre_roll.iter().cloned().collect();
        all.extend(samples.iter().cloned());
        vad.accept_waveform(all);

        if vad.is_speech() {
            if !speaking {
                log::info!(">>> 语音开始");
                speaking = true;
                buffer.extend(pre_roll.iter().cloned());
            }
            buffer.extend(samples);
            silence_count = 0;
        } else if speaking {
            silence_count += 1;
            buffer.extend(samples);

            if silence_count > SILENCE_THRESHOLD {
                log::info!(
                    "<<< 语音结束，{:?}",
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
