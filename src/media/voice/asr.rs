use crate::media::voice::VAD_WINDOW_SIZE;
use cpal::traits::DeviceTrait;
use cpal::{Device, Stream};
use sherpa_rs::sense_voice::SenseVoiceConfig;
use sherpa_rs::silero_vad::{SileroVad, SileroVadConfig};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::{mpsc, Arc};

/// 读取原始语音信息
///
/// # Arguments
///
/// * `device`: 麦克风设备
/// * `volume`: 显示的实时音量
/// * `audio_tx`: 原始音频数据发送通道
///
/// returns: Result<Stream, Error>
///
/// # Examples
///
/// ```
///
/// ```
pub fn build_audio_stream(
    device: &Device,
    volume: Arc<AtomicI32>,
    audio_tx: SyncSender<Vec<f32>>,
) -> anyhow::Result<Stream> {
    let config = device.default_input_config()?;
    let channels = config.channels() as usize;
    let volume_clone = volume.clone();

    let stream_config: cpal::StreamConfig = cpal::StreamConfig {
        channels: config.channels(),
        sample_rate: config.sample_rate(),
        buffer_size: cpal::BufferSize::Fixed(512),
    };

    Ok(device.build_input_stream(
        &stream_config,
        move |data: &[f32], _: &_| {
            // 计算音量
            let sum: f32 = data.iter().map(|&s| s * s).sum();
            let rms = (sum / data.len() as f32).sqrt();
            volume_clone.store((rms * 100.0).min(100.0) as i32, Ordering::Relaxed);

            let mono: Vec<f32> = if channels == 2 {
                // 合并双声道
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

/// 音频识别线程, 收到的每一包数据先丢入vad,
/// 再讲话时就保存数据, 说完再识别
///
/// # Arguments
///
/// * `model_path`: 语音识别模型地址
/// * `audio_rx`: 待识别音频数据
/// * `result_tx`: 返回识别结果
///
/// returns: ()
///
/// # Examples
///
/// ```
///
/// ```
pub fn recognition_thread(
    sense_voice_model_path: String,
    silero_vad_model_path: String,
    audio_rx: Receiver<Vec<f32>>,
    _result_tx: mpsc::Sender<String>,
) {
    let _config = SenseVoiceConfig {
        model: sense_voice_model_path,
        tokens: "".into(), // Will be auto-detected from model directory
        #[cfg(target_os = "windows")]
        provider: Some("cpu".into()),
        ..Default::default()
    };

    // let mut recognizer = match SenseVoiceRecognizer::new(config) {
    //     Ok(r) => r,
    //     Err(e) => {
    //         log::error!("Failed to create SenseVoice recognizer: {:?}", e);
    //         return;
    //     }
    // };
    let mut buffer = Vec::new();
    // 加载静音检测模型
    let vad_config = SileroVadConfig {
        model: silero_vad_model_path,
        window_size: VAD_WINDOW_SIZE,
        ..Default::default()
    };
    let mut vad = SileroVad::new(vad_config, 60.0 * 10.0).unwrap();

    for samples in audio_rx {
        vad.accept_waveform(samples.clone());
        if vad.is_speech() {
            buffer.extend(samples);
        } else if !buffer.is_empty() {
            log::info!("received a audio msg, len: {}", buffer.len());
        }
    }
}
