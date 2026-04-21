#![allow(dead_code)]

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::Device;
use sherpa_onnx::{GenerationConfig, OfflineTts, OfflineTtsConfig, OfflineTtsVitsModelConfig};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Callback type for TTS progress reporting
type TtsCallback = Option<Box<dyn FnMut(&[f32], f32) -> bool + Send + 'static>>;

/// TTS audio output data
#[derive(Debug, Clone)]
pub struct TtsAudio {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,
}

/// TTS handler for synthesizing speech using sherpa-onnx VITS
/// Wrapped in Arc<Mutex<>> to ensure thread safety since OfflineTts is not Sync
pub struct TtsHandler {
    tts: Arc<Mutex<OfflineTts>>,
    sample_rate: i32,
}

unsafe impl Send for TtsHandler {}
unsafe impl Sync for TtsHandler {}

impl TtsHandler {
    /// Create a new TtsHandler
    ///
    /// # Arguments
    /// * `model_path` - Path to the VITS TTS model (.onnx)
    /// * `tokens_path` - Path to the tokens file (.txt)
    /// * `lexicon_path` - Path to the lexicon file (.txt)
    pub fn new(
        model_path: impl AsRef<Path>,
        tokens_path: impl AsRef<Path>,
        lexicon_path: impl AsRef<Path>,
    ) -> Result<Self> {
        let model_path = model_path.as_ref();
        let tokens_path = tokens_path.as_ref();
        let lexicon_path = lexicon_path.as_ref();

        if !model_path.exists() {
            anyhow::bail!("Model path {:?} does not exist", model_path);
        }
        if !tokens_path.exists() {
            anyhow::bail!("Tokens path {:?} does not exist", tokens_path);
        }
        if !lexicon_path.exists() {
            anyhow::bail!("Lexicon path {:?} does not exist", lexicon_path);
        }

        let config = OfflineTtsConfig {
            model: sherpa_onnx::OfflineTtsModelConfig {
                vits: OfflineTtsVitsModelConfig {
                    model: Some(model_path.to_string_lossy().to_string()),
                    tokens: Some(tokens_path.to_string_lossy().to_string()),
                    lexicon: Some(lexicon_path.to_string_lossy().to_string()),
                    length_scale: 1.0,
                    ..Default::default()
                },
                num_threads: 2,
                debug: false,
                ..Default::default()
            },
            ..Default::default()
        };

        let tts = OfflineTts::create(&config).ok_or_else(|| anyhow!("Failed to create TTS"))?;

        let sample_rate = tts.sample_rate();
        log::info!("TTS initialized with sample rate: {}", sample_rate);

        Ok(Self {
            tts: Arc::new(Mutex::new(tts)),
            sample_rate,
        })
    }

    /// Synthesize text to audio
    ///
    /// # Arguments
    /// * `text` - Text to synthesize
    /// * `speed` - Speech speed (1.0 = normal, 0.5 = half speed, 2.0 = double speed)
    pub fn synthesize(&self, text: &str, speed: f32) -> Result<TtsAudio> {
        let gen_config = GenerationConfig {
            sid: 0,
            speed,
            ..Default::default()
        };

        let tts = self
            .tts
            .lock()
            .map_err(|_| anyhow!("TTS is not available"))?;
        let callback: TtsCallback = Some(Box::new(|_chunk: &[f32], _progress: f32| true));
        let audio = tts
            .generate_with_config(text, &gen_config, callback)
            .ok_or_else(|| anyhow!("TTS generation failed"))?;

        Ok(TtsAudio {
            samples: audio.samples().to_vec(),
            sample_rate: audio.sample_rate() as u32,
            channels: 1,
        })
    }

    /// Synthesize text to audio with streaming callback
    ///
    /// # Arguments
    /// * `text` - Text to synthesize
    /// * `speed` - Speech speed (1.0 = normal, 0.5 = half speed, 2.0 = double speed)
    /// * `callback` - Called for each audio chunk generated, parameter is audio data
    pub fn synthesize_streaming<F>(&self, text: &str, speed: f32, mut callback: F) -> Result<()>
    where
        F: FnMut(&[f32]) + Send + 'static,
    {
        let gen_config = GenerationConfig {
            sid: 0,
            speed,
            ..Default::default()
        };

        let tts = self
            .tts
            .lock()
            .map_err(|_| anyhow!("TTS is not available"))?;

        let stream_callback: TtsCallback = Some(Box::new(move |chunk: &[f32], _progress: f32| {
            callback(chunk);
            true // continue generation
        }));

        tts.generate_with_config(text, &gen_config, stream_callback)
            .ok_or_else(|| anyhow!("TTS generation failed"))?;

        Ok(())
    }

    /// Get the TTS sample rate
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate as u32
    }
}

/// TTS player for playing audio through speakers
pub struct TtsPlayer {
    device: Device,
    is_playing: Arc<AtomicBool>,
}

/// Handle for streaming audio playback
pub struct StreamPlayerHandle {
    pub sample_rate: u32,
    buffer: Arc<Mutex<Vec<f32>>>,
    total_written: Arc<AtomicUsize>,
    consumed: Arc<AtomicUsize>,
}

impl StreamPlayerHandle {
    /// Add a chunk of audio samples to the buffer
    pub fn write_chunk(&self, chunk: &[f32]) {
        let Some(mut buffer) = self.buffer.lock().ok() else {
            log::warn!("lock StreamPlayerHandle.buffer failed");
            return;
        };
        buffer.extend_from_slice(chunk);
        self.total_written.fetch_add(chunk.len(), Ordering::SeqCst);
    }

    /// Check if all samples have been consumed
    pub fn is_done(&self) -> bool {
        let total = self.total_written.load(Ordering::SeqCst);
        let consumed = self.consumed.load(Ordering::SeqCst);
        consumed >= total && total > 0
    }
}

impl TtsPlayer {
    /// Create a new TtsPlayer using the default output device
    pub fn new() -> Result<Self> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or_else(|| anyhow!("No default audio output device found"))?;

        log::info!("TTS output device: {:?}", device.description());

        Ok(Self {
            device,
            is_playing: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Play TTS audio
    pub fn play(&self, audio: &TtsAudio) -> Result<()> {
        let config = cpal::StreamConfig {
            channels: audio.channels,
            sample_rate: audio.sample_rate,
            buffer_size: cpal::BufferSize::Default,
        };

        let is_playing = self.is_playing.clone();
        is_playing.store(true, Ordering::SeqCst);

        let samples = audio.samples.clone();
        let sample_count = samples.len();

        let stream = self.device.build_output_stream(
            &config,
            crate::media::voice::write_audio_callback(samples),
            |err| log::error!("TTS stream error: {}", err),
            None,
        )?;

        stream.play()?;

        // Wait for playback to complete
        let duration_ms = (sample_count as u64 * 1000) / (audio.sample_rate as u64);
        std::thread::sleep(std::time::Duration::from_millis(duration_ms + 100));

        is_playing.store(false, Ordering::SeqCst);

        Ok(())
    }

    /// Check if currently playing
    pub fn is_playing(&self) -> bool {
        self.is_playing.load(Ordering::SeqCst)
    }

    /// Start streaming audio playback
    ///
    /// Returns a `StreamPlayerHandle` that can be used to write audio chunks.
    /// The handle's `write_chunk` method is thread-safe and can be called from any thread.
    pub fn start_streaming(&self, sample_rate: u32) -> Result<StreamPlayerHandle> {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let consumed = Arc::new(AtomicUsize::new(0));
        let total_written = Arc::new(AtomicUsize::new(0));

        let buffer_clone = buffer.clone();
        let consumed_clone = consumed.clone();

        let config = cpal::StreamConfig {
            channels: 1,
            sample_rate,
            buffer_size: cpal::BufferSize::Default,
        };

        let stream = self.device.build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let mut buffer = buffer_clone.lock().unwrap();
                let consumed_idx = consumed_clone.load(Ordering::SeqCst);

                for (i, sample) in data.iter_mut().enumerate() {
                    if consumed_idx + i < buffer.len() {
                        *sample = buffer[consumed_idx + i];
                    } else {
                        *sample = 0.0;
                    }
                }

                consumed_clone.fetch_add(data.len(), Ordering::SeqCst);

                // Trim buffer to prevent memory growth
                if consumed_idx > 44100 {
                    buffer.drain(0..consumed_idx);
                    consumed_clone.store(0, Ordering::SeqCst);
                }
            },
            |err| log::error!("TTS streaming error: {}", err),
            None,
        )?;

        stream.play()?;

        Ok(StreamPlayerHandle {
            sample_rate,
            buffer,
            total_written,
            consumed,
        })
    }
}

impl Default for TtsPlayer {
    fn default() -> Self {
        // unwrap is safe here as it only fails if no audio device exists
        Self::new().unwrap()
    }
}

impl TtsAudio {
    /// Save audio to a WAV file
    ///
    /// # Arguments
    /// * `path` - Output WAV file path
    pub fn save_wav(&self, path: impl AsRef<Path>) -> Result<()> {
        use std::io::Write;

        let path = path.as_ref();
        let sample_rate = self.sample_rate;
        let num_channels = self.channels;
        let bits_per_sample: u16 = 16;
        let num_samples = self.samples.len() as u32;
        let byte_rate = sample_rate * num_channels as u32 * bits_per_sample as u32 / 8;
        let block_align = num_channels * bits_per_sample / 8;
        let data_size = num_samples * num_channels as u32 * bits_per_sample as u32 / 8;

        let mut file = std::fs::File::create(path)?;

        // RIFF header
        file.write_all(b"RIFF")?;
        file.write_all(&(36 + data_size).to_le_bytes())?;
        file.write_all(b"WAVE")?;

        // fmt sub-chunk
        file.write_all(b"fmt ")?;
        file.write_all(&16u32.to_le_bytes())?;
        file.write_all(&1u16.to_le_bytes())?;
        file.write_all(&num_channels.to_le_bytes())?;
        file.write_all(&sample_rate.to_le_bytes())?;
        file.write_all(&byte_rate.to_le_bytes())?;
        file.write_all(&block_align.to_le_bytes())?;
        file.write_all(&bits_per_sample.to_le_bytes())?;

        // data sub-chunk
        file.write_all(b"data")?;
        file.write_all(&data_size.to_le_bytes())?;

        // Convert f32 samples to i16 and write
        for &sample in &self.samples {
            let sample_i16 = (sample * 32767.0).clamp(-32768.0, 32767.0) as i16;
            file.write_all(&sample_i16.to_le_bytes())?;
        }

        log::info!("Saved WAV file: {:?}", path);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_manager::ModelManager;

    #[test]
    fn test_tts_synthesize_and_save() {
        eprintln!("Starting TTS test...");

        // Skip if TTS models not available
        let model_path = match ModelManager::global().get("vits_tts") {
            Some(p) => {
                eprintln!("Found vits_tts model: {:?}", p);
                p
            }
            None => {
                eprintln!("VITS TTS model not found, skipping test");
                return;
            }
        };
        let tokens_path = match ModelManager::global().get("vits_tts_tokens") {
            Some(p) => {
                eprintln!("Found vits_tts_tokens: {:?}", p);
                p
            }
            None => {
                eprintln!("VITS tokens not found, skipping test");
                return;
            }
        };
        let lexicon_path = match ModelManager::global().get("vits_tts_lexicon") {
            Some(p) => {
                eprintln!("Found vits_tts_lexicon: {:?}", p);
                p
            }
            None => {
                eprintln!("VITS lexicon not found, skipping test");
                return;
            }
        };

        eprintln!("Creating TtsHandler...");
        let handler = TtsHandler::new(&model_path, &tokens_path, &lexicon_path)
            .expect("Failed to create TtsHandler");

        eprintln!("Synthesizing speech...");
        let audio = handler
            .synthesize("你好，这是测试语音。", 1.0)
            .expect("Failed to synthesize");

        eprintln!("Saving WAV file...");
        let output_path = "./assets/audio/tts_test_output.wav";
        audio
            .save_wav(output_path)
            .expect("Failed to save WAV file");

        // Verify file exists and has content
        let metadata = std::fs::metadata(output_path).expect("Failed to get file metadata");
        assert!(
            metadata.len() > 44,
            "WAV file should be larger than header size"
        );

        eprintln!(
            "Generated WAV file: {} ({} bytes)",
            output_path,
            metadata.len()
        );
    }
}
