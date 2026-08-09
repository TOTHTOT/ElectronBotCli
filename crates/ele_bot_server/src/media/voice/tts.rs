//! TTS (Text-to-Speech) 播放层
//!
//! 提供基于 sherpa-onnx VITS 的文本合成, 通过 rodio 推到扬声器.
//!
//! **所有权约定**: 输出流常驻在 `TtsPlayer` 内部的 rodio `MixerDeviceSink`
//! 上, 随 `TtsPlayer` Drop 释放; `SharedState::rebuild_voice` 切音频设备时
//! 通过 `Arc::drop` 链释放旧 device 句柄, 不会出现 leaked stream 残留.
//! 样本的位宽/声道/采样率转换全部由 rodio mixer 自动完成, 本层不含任何
//! 设备型号或平台的特判分支.

use anyhow::{anyhow, Result};
use cpal::traits::DeviceTrait;
use rodio::buffer::SamplesBuffer;
use rodio::{queue, ChannelCount, DeviceSinkBuilder, MixerDeviceSink, Player, SampleRate};
use sherpa_onnx::{GenerationConfig, OfflineTts, OfflineTtsConfig, OfflineTtsVitsModelConfig};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
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
///
/// Wrapped in `Arc<Mutex<>>` to ensure thread safety since sherpa-onnx's
/// `OfflineTts` is not `Sync`. 因为 `Arc<Mutex<T>>` 在 `T: Send` 时天然是
/// `Send + Sync`, 不需要 (也不允许) 手写 `unsafe impl Send/Sync`.
#[derive(Clone)]
pub struct TtsHandler {
    tts: Arc<Mutex<OfflineTts>>,
    sample_rate: i32,
}

impl TtsHandler {
    /// 创建 TTS handler, 加载 sherpa-onnx VITS 模型到内存.
    ///
    /// # 参数
    /// * `model_path` - VITS 模型文件 (.onnx)
    /// * `tokens_path` - tokens 文件 (.txt)
    /// * `lexicon_path` - lexicon 文件 (.txt)
    ///
    /// # 边界
    /// 三个文件都必须存在 (运行时 bail 不会 fallback). sherpa-onnx 的
    /// `OfflineTts` 不是 `Sync`, 本类型用 `Arc<Mutex<T>>` 包装保证跨线程.
    /// 因为 `Arc<Mutex<T>>` 天然 Send+Sync, 本类型不再写 `unsafe impl Send/Sync`.
    ///
    /// # Examples
    /// ```rust,ignore
    /// let handler = TtsHandler::new("model.onnx", "tokens.txt", "lexicon.txt")?;
    /// ```
    pub fn new(
        model_path: impl AsRef<Path>,
        tokens_path: impl AsRef<Path>,
        lexicon_path: impl AsRef<Path>,
    ) -> Result<Self> {
        let model_path = model_path.as_ref();
        let tokens_path = tokens_path.as_ref();
        let lexicon_path = lexicon_path.as_ref();

        if !model_path.exists() {
            anyhow::bail!("Model path {model_path:?} does not exist");
        }
        if !tokens_path.exists() {
            anyhow::bail!("Tokens path {tokens_path:?} does not exist");
        }
        if !lexicon_path.exists() {
            anyhow::bail!("Lexicon path {lexicon_path:?} does not exist");
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
        log::info!("TTS initialized with sample rate: {sample_rate}");

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

    /// 使用流式回调合成文本到音频
    ///
    /// # 参数
    /// * `text` - 要合成的文本
    /// * `speed` - 语速 (1.0 = 正常, 0.5 = 半速, 2.0 = 倍速)
    /// * `callback` - 每个音频块生成时调用，参数为音频数据
    pub fn synthesize_streaming<F>(&self, text: &str, speed: f32, mut callback: F) -> Result<()>
    where
        F: FnMut(&[f32], f32) + Send + 'static,
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

        let stream_callback: TtsCallback = Some(Box::new(move |chunk: &[f32], progress: f32| {
            callback(chunk, progress);
            true // continue generation
        }));

        tts.generate_with_config(text, &gen_config, stream_callback)
            .ok_or_else(|| anyhow!("TTS generation failed"))?;

        Ok(())
    }

    /// Get the TTS sample rate
    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate as u32
    }
}

/// TTS 播放器: 把合成音频推到扬声器.
///
/// 内部用 rodio `MixerDeviceSink` 持有一条常驻输出流 (创建时
/// `open_sink_or_fallback` 按设备真实能力开流), 样本的位宽/声道/采样率
/// 转换全部由 rodio mixer 自动完成 — PCM2912A 这类只收 S16_LE 的 USB
/// 声卡自动适配, 无需设备特判.
///
/// 设备句柄随 `TtsPlayer` Drop 释放, `SharedState::rebuild_voice` 切
/// 音频设备时的 RAII 释放链保持不变 (见 `docs/voice-hot-swap.md`).
pub struct TtsPlayer {
    sink: MixerDeviceSink,
    /// 播放增益 (f32 bits, 1.0 = 原始音量). 原子量共享自 `VoiceManager`,
    /// `set_config` 调音量时热更新, 下一次播放即生效, 无需重建输出流.
    gain: Arc<AtomicU32>,
}

/// 流式 TTS 播放句柄.
///
/// 内部是 rodio queue 源 (合成线程 `write_chunk` 追加) + 一个 `Player`
/// (mixer 消费). 调用方**必须**持有本句柄到 `is_done()` 返回 `true`
/// 再 Drop — 提前 Drop 会截断仍在队列里的音频.
pub struct StreamPlayerHandle {
    /// 播放端: append 了 queue 输出源的 rodio `Player`
    player: Player,
    /// 合成端: `write_chunk` 往这里追加样本
    queue_input: Arc<queue::SourcesQueueInput>,
    /// 合成完成标志 (合成线程 set)
    synthesis_done: Arc<AtomicBool>,
    /// 源采样率, `write_chunk` 构造 `SamplesBuffer` 用
    sample_rate: SampleRate,
}

impl StreamPlayerHandle {
    /// 把一个音频块追加到播放队列. 线程安全, 可在合成线程里调.
    ///
    /// 声道/采样率/位宽转换由 rodio mixer 自动完成, 这里只按源格式
    /// (mono / 合成采样率) 描述 chunk.
    pub fn write_chunk(&self, chunk: &[f32], progress: f32) {
        self.queue_input.append(SamplesBuffer::new(
            // TTS 输出恒为 mono
            ChannelCount::new(1).expect("1 != 0"),
            self.sample_rate,
            chunk.to_vec(),
        ));
        log::info!("deal progress: {progress}");
    }

    /// 标记合成完成. queue 不再保持 alive — 队列消费空后 `Player` 即视为
    /// 播放结束, 调用方通过 `is_done()` 轮询得知.
    pub fn mark_synthesis_done(&self) {
        self.synthesis_done.store(true, Ordering::SeqCst);
        self.queue_input.set_keep_alive_if_empty(false);
    }

    /// 是否播放完成 (合成结束 + 队列已消费完).
    #[must_use]
    pub fn is_done(&self) -> bool {
        self.synthesis_done.load(Ordering::SeqCst) && self.player.empty()
    }
}

impl TtsPlayer {
    /// 创建 TTS 播放器, 打开指定的输出设备.
    ///
    /// # 参数
    /// * `output_device_name` - cpal 设备名. 空字符串 `""` 表示系统默认设备.
    /// * `output_device_id` - cpal `DeviceId` 序列化的稳定标识. 与
    ///   `output_device_name` 配套传入; `None` 或空时按 name 兜底.
    ///
    /// # 边界
    /// 找不到设备或开流失败时返回 `Err`. `open_sink_or_fallback` 内部按
    /// 设备真实能力遍历 supported configs 选配置开流 — PCM2912A 这类只收
    /// S16_LE 的 USB 声卡自动落到 S16 配置, 无需设备特判. 输出流常驻到
    /// `TtsPlayer` Drop.
    ///
    /// # Examples
    /// ```rust,ignore
    /// let player = TtsPlayer::new("", None)?; // 默认设备
    /// let player = TtsPlayer::new("Speakers (Realtek)", None)?;
    /// let player = TtsPlayer::new("", Some("{0.0.0...}"))?; // 按 id 优先
    /// ```
    pub fn new(
        output_device_name: &str,
        output_device_id: Option<&str>,
        gain: Arc<AtomicU32>,
    ) -> Result<Self> {
        let device = crate::media::voice::find_output_device(output_device_name, output_device_id)
            .ok_or_else(|| anyhow!("No audio output device found"))?;

        log::info!("TTS output device: {:?}", device.description());

        // rodio 0.22 依赖 cpal 0.17, 与本项目直依赖的 cpal 0.18 是两个
        // 不兼容类型, `Device` 无法直接互传 — 按设备名在 rodio 侧重新
        // 枚举出同一个物理设备.
        let device_name = device
            .description()
            .map(|d| d.name().to_string())
            .unwrap_or_default();
        let rodio_device = Self::find_rodio_device(&device_name)?;

        let sink = DeviceSinkBuilder::from_device(rodio_device)
            .map_err(|e| anyhow!("failed to init audio output stream: {e}"))?
            .open_sink_or_fallback()
            .map_err(|e| anyhow!("failed to open audio output stream: {e}"))?;
        let sink_config = sink.config();
        log::info!(
            "TTS output stream opened: {} ch / {} Hz / {:?}",
            sink_config.channel_count(),
            sink_config.sample_rate(),
            sink_config.sample_format()
        );

        Ok(Self { sink, gain })
    }

    /// 在 rodio (cpal 0.17) 侧按设备名找回同一个物理输出设备.
    ///
    /// rodio 0.22 依赖 cpal 0.17, 与本项目直依赖的 cpal 0.18 类型不兼容,
    /// `Device` 无法跨版本传递, 只能按名匹配 (重名时取第一个). 匹配不到
    /// 时回退 rodio 默认输出设备并 warn — 播错设备比直接失败好排查.
    fn find_rodio_device(device_name: &str) -> Result<rodio::Device> {
        // `DeviceTrait as _`: 只把 0.17 的方法带进作用域, 不与顶部
        // cpal 0.18 的 DeviceTrait 重名冲突
        use rodio::cpal::traits::{DeviceTrait as _, HostTrait};

        let host = rodio::cpal::default_host();
        if !device_name.is_empty() {
            if let Ok(devices) = host.output_devices() {
                if let Some(dev) = devices.into_iter().find(|d| {
                    d.description()
                        .ok()
                        .is_some_and(|desc| desc.name() == device_name)
                }) {
                    return Ok(dev);
                }
            }
            log::warn!("TTS device {device_name:?} not found on rodio side, fallback to default");
        }
        host.default_output_device()
            .ok_or_else(|| anyhow!("No audio output device found (rodio side)"))
    }

    /// 把 TTS 音频推完整个 buffer. 阻塞到 rodio mixer 真正消费完全部样本
    /// 才返回 — 替代旧实现里手写计数器轮询 / wall-clock sleep 估算时长.
    pub fn play(&self, audio: TtsAudio) -> Result<()> {
        let channels = ChannelCount::new(audio.channels)
            .ok_or_else(|| anyhow!("invalid channel count: {}", audio.channels))?;
        let sample_rate = SampleRate::new(audio.sample_rate)
            .ok_or_else(|| anyhow!("invalid sample rate: {}", audio.sample_rate))?;

        let player = Player::connect_new(self.sink.mixer());
        player.set_volume(self.current_gain());
        // mixer 自动包 UniformSourceIterator 做声道+采样率+位宽转换
        player.append(SamplesBuffer::new(channels, sample_rate, audio.samples));
        player.sleep_until_end();

        Ok(())
    }

    /// 启动流式播放. 返回的 `StreamPlayerHandle` 持有 rodio `Player` +
    /// queue 输入端, 调用方必须持有到 `is_done()` 返回 `true` 再 Drop —
    /// 提前 Drop 会截断仍在队列里的音频.
    ///
    /// 调用方模式 (`VoiceManager::speak_streaming`):
    /// 1. `let handle = player.start_streaming(...)?;`
    /// 2. `let handle = Arc::new(handle);` 给合成线程一份 clone
    /// 3. 合成线程 `handle.write_chunk(...)` + `handle.mark_synthesis_done()`
    /// 4. 调用方 `while !handle.is_done() { sleep(50ms) }`
    /// 5. `handle` 离开作用域 → `Player` Drop → 停止消费 queue
    ///
    /// # 边界
    /// - 设备开流失败在 `TtsPlayer::new` 就报了, 本方法只对非法采样率
    ///   返回 `Err`
    /// - 切音频设备 (`SharedState::rebuild_voice`) 触发时, 仍在播放的句柄
    ///   要等 `is_done()` 后才被 drop — 这条延迟路径在本 change 范围内不解决
    ///   ("切设备立即打断 TTS" 是未来 change).
    ///
    /// # Examples
    /// ```rust,ignore
    /// let handle = player.start_streaming(22050)?;
    /// // ... 合成线程 write_chunk + mark_synthesis_done
    /// while !handle.is_done() { std::thread::sleep(...); }
    /// drop(handle); // 释放 Player, 停止消费 queue
    /// ```
    pub fn start_streaming(&self, sample_rate: u32) -> Result<StreamPlayerHandle> {
        let sample_rate = SampleRate::new(sample_rate)
            .ok_or_else(|| anyhow!("invalid sample rate: {sample_rate}"))?;

        // keep_alive_if_empty = true: 第一块样本还没合成出来时 queue 为空,
        // 也不让 Player 提前判定结束.
        let (queue_input, queue_output) = queue::queue(true);
        let player = Player::connect_new(self.sink.mixer());
        player.set_volume(self.current_gain());
        player.append(queue_output);

        Ok(StreamPlayerHandle {
            player,
            queue_input,
            synthesis_done: Arc::new(AtomicBool::new(false)),
            sample_rate,
        })
    }
    /// 读当前播放增益 (原子量存 f32 bits).
    fn current_gain(&self) -> f32 {
        f32::from_bits(self.gain.load(Ordering::Relaxed))
    }
}

impl Default for TtsPlayer {
    fn default() -> Self {
        // unwrap is safe here as it only fails if no audio device exists
        Self::new("", None, Arc::new(AtomicU32::new(1.0f32.to_bits()))).unwrap()
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
        let byte_rate = sample_rate * u32::from(num_channels) * u32::from(bits_per_sample) / 8;
        let block_align = num_channels * bits_per_sample / 8;
        let data_size = num_samples * u32::from(num_channels) * u32::from(bits_per_sample) / 8;

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

        log::info!("Saved WAV file: {path:?}");
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
        // 锚到 workspace 根: cargo test 的 cwd 是 crate 目录, 相对路径会落空
        let output_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/audio/tts_test_output.wav"
        );
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
