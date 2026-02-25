use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Stream};
use rodio::{OutputStream, Sink};
use sherpa_rs::sense_voice::{SenseVoiceConfig, SenseVoiceRecognizer};
use sherpa_rs::tts::{TtsAudio, VitsTts, VitsTtsConfig};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::mpsc::{Receiver, SyncSender};
use std::sync::{mpsc, Arc};
use std::thread;

#[allow(dead_code)]
pub struct VoiceManager {
    _stream: Stream,
    volume: Arc<AtomicI32>,
    _tts: Option<VitsTts>,
    _stream_handle: Option<OutputStream>,
    _sink: Option<Sink>,
}

#[allow(dead_code)]
impl VoiceManager {
    pub fn new(
        model_path: &str,
        tts_model_path: &str,
        tts_tokens_path: &str,
        speech_name: &str,
        result_tx: mpsc::Sender<String>,
    ) -> Result<Self> {
        let device = find_input_device(speech_name)?;
        let config = get_input_config(&device)?;

        let volume = Arc::new(AtomicI32::new(0));

        // Initialize TTS
        let tts_config = VitsTtsConfig {
            model: tts_model_path.into(),
            tokens: tts_tokens_path.into(),
            ..Default::default()
        };
        let tts = VitsTts::new(tts_config);

        // Initialize audio stream for recognition
        let (audio_tx, audio_rx) = mpsc::sync_channel::<Vec<i16>>(4);

        let stream = build_audio_stream(&device, &config, volume.clone(), audio_tx)?;
        stream.play()?;

        // Start recognition thread - clone model_path to avoid lifetime issues
        let model_path_owned = model_path.to_string();
        thread::spawn(move || {
            recognition_thread(&model_path_owned, audio_rx, result_tx);
        });

        Ok(Self {
            _stream: stream,
            volume,
            _tts: Some(tts),
            _stream_handle: None,
            _sink: None,
        })
    }

    /// Play text-to-speech audio
    #[allow(dead_code)]
    pub fn speak(&mut self, text: &str) -> Result<()> {
        let tts = self
            ._tts
            .as_mut()
            .ok_or_else(|| anyhow!("TTS not initialized"))?;

        // Generate audio (sid = 0, speed = 1.0)
        let audio: TtsAudio = tts
            .create(text, 0, 1.0)
            .map_err(|e| anyhow!("TTS generation failed: {:?}", e))?;

        // Play audio using rodio
        self.play_audio(&audio)?;

        Ok(())
    }

    fn play_audio(&mut self, audio: &TtsAudio) -> Result<()> {
        // Get or create output stream
        if self._stream_handle.is_none() {
            let (stream, stream_handle) = OutputStream::try_default()?;
            let sink = Sink::try_new(&stream_handle)?;
            self._stream_handle = Some(stream);
            self._sink = Some(sink);
        }

        let sink = self
            ._sink
            .as_mut()
            .ok_or_else(|| anyhow!("Failed to get sink"))?;

        // Convert i16 samples to f32 and play
        let samples: Vec<f32> = audio.samples.iter().map(|&s| s / i16::MAX as f32).collect();

        // Create source from samples
        let source = rodio::buffer::SamplesBuffer::new(
            1, // mono
            audio.sample_rate,
            samples,
        );

        sink.append(source);
        sink.play();

        Ok(())
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
    volume: Arc<AtomicI32>,
    audio_tx: SyncSender<Vec<i16>>,
) -> Result<Stream> {
    let channels = config.channels() as usize;
    let volume_clone = volume.clone();

    let stream_config: cpal::StreamConfig = cpal::StreamConfig {
        channels: config.channels(),
        sample_rate: config.sample_rate(),
        buffer_size: cpal::BufferSize::Default,
    };

    device
        .build_input_stream(
            &stream_config,
            move |data: &[f32], _: &_| {
                // Calculate RMS volume
                let sum: f32 = data.iter().map(|&s| s * s).sum();
                let rms = (sum / data.len() as f32).sqrt();
                volume_clone.store((rms * 100.0).min(100.0) as i32, Ordering::Relaxed);

                // Convert to mono i16
                let mono: Vec<f32> = if channels == 2 {
                    data.chunks(2).map(|c| (c[0] + c[1]) / 2.0).collect()
                } else {
                    data.to_vec()
                };

                let samples: Vec<i16> =
                    mono.iter().map(|&s| (s * i16::MAX as f32) as i16).collect();

                let _ = audio_tx.send(samples);
            },
            |e| log::error!("Audio stream error: {}", e),
            None,
        )
        .map_err(|e| anyhow!("Failed to build input stream: {}", e))
}

/// Recognition thread - processes audio chunks using sherpa-rs SenseVoice
fn recognition_thread(
    model_path: &str,
    audio_rx: Receiver<Vec<i16>>,
    result_tx: mpsc::Sender<String>,
) {
    // Initialize SenseVoice recognizer
    let config = SenseVoiceConfig {
        model: model_path.into(),
        tokens: "".into(), // Will be auto-detected from model directory
        provider: Some("cpu".into()),
        ..Default::default()
    };

    let mut recognizer = match SenseVoiceRecognizer::new(config) {
        Ok(r) => r,
        Err(e) => {
            log::error!("Failed to create SenseVoice recognizer: {:?}", e);
            return;
        }
    };

    const SAMPLE_RATE: u32 = 16000;
    const CHUNK_SIZE: usize = 1600; // 100ms at 16kHz

    let mut buffer = Vec::new();

    for samples in audio_rx {
        buffer.extend(samples);

        // Process in chunks
        while buffer.len() >= CHUNK_SIZE {
            let chunk: Vec<i16> = buffer.drain(..CHUNK_SIZE).collect();

            // Convert i16 to f32 for sherpa-rs
            let float_samples: Vec<f32> =
                chunk.iter().map(|&s| s as f32 / i16::MAX as f32).collect();

            // Transcribe
            let result = recognizer.transcribe(SAMPLE_RATE, &float_samples);

            if !result.text.is_empty() {
                log::info!("Recognition result: {}", result.text);
                let _ = result_tx.send(result.text);
            }
        }
    }
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
    device: &cpal::Device,
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
    std::thread::sleep(std::time::Duration::from_millis(duration_ms as u64 + 50));

    Ok(())
}
