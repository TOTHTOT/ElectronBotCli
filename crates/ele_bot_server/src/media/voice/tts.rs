//! TTS (Text-to-Speech) 播放层
//!
//! 提供基于 sherpa-onnx VITS 的文本合成, 通过 cpal `OutputStream` 推到扬声器.
//!
//! **所有权约定**: cpal `OutputStream` 必须绑到返回的 handle (`StreamPlayerHandle`
//! 或 `OwnedOutputStream` + 临时变量) 上, 由调用方持有到播放结束再 Drop. 这保证
//! `SharedState::rebuild_voice` 在用户切音频设备时能通过 `Arc::drop` 链释放旧
//! device 句柄, 不会出现 leaked stream 残留. **禁止** `mem::forget` / `Box::leak`
//! / 全局单例绕开 Drop.

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, StreamTrait};
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

/// cpal `OutputStream` 的 RAII 包装.
///
/// **为什么需要**: cpal Stream 必须保持存活直到播放结束, 但 `cpal::Stream`
/// 在 cpal 5.x 已自带 drop 释放底层 handle, 这里再显式 `pause()` 是为了:
/// 1. 让 `rebuild_voice` 在切音频设备时, 旧 `VoiceManager` 通过 `Arc::drop`
///    链触发本类型 Drop, 旧 device 句柄尽快释放 (不被独占锁卡住)
/// 2. 防止 cpal Stream 被 `mem::forget` / `Box::leak` 绕开 Drop 路径
///
/// **不要** 用 `mem::forget` 等手段让 Stream 脱离所有权链, 否则热重建切设备时
/// 旧 device 句柄会泄漏 (见 `docs/voice-hot-swap.md`).
pub struct OwnedOutputStream {
    stream: cpal::Stream,
}

impl Drop for OwnedOutputStream {
    fn drop(&mut self) {
        // 显式 pause: 即使 cpal 自己 Drop 时会停流, 主动调一次能缩短
        // 旧设备被独占占用时的释放窗口. 失败吞掉 (设备可能已经消失).
        let _ = self.stream.pause();
    }
}

/// TTS 播放器, 把 TTS 合成出的音频推到扬声器 (cpal `OutputStream`).
///
/// 持有 cpal `Device`, 具体到播放时的 `OutputStream` 通过 `play` / `start_streaming`
/// 返回, 调用方负责持有到结束. 这样 `SharedState::rebuild_voice` 在切音频设备
/// drop 旧 `VoiceManager` → drop 旧 `TtsPlayer` 时, 还在跑的 stream 也能通过
/// 链上的 `Arc::drop` 一并释放旧 device 句柄.
pub struct TtsPlayer {
    device: Device,
    /// 创建时试开流确定的采样格式 (F32 或 I16), 播放时直接使用.
    ///
    /// 不能用 `supported_output_configs` 查询代替实测 — PCM2912A
    /// (sysdefault:CARD=CODEC) 查询返回支持 F32, 实际 `build_output_stream`
    /// 才报 "stream configuration is not supported".
    sample_format: cpal::SampleFormat,
}

/// cpal 输出回调的装箱类型 (f32 / i16 两个变体, `build_stream` 用)
type F32Callback = Box<dyn FnMut(&mut [f32], &cpal::OutputCallbackInfo) + Send>;
type I16Callback = Box<dyn FnMut(&mut [i16], &cpal::OutputCallbackInfo) + Send>;

/// 流式播放回调的公共体: 从共享 buffer head 消费样本填入 cpal 输出
/// buffer (经 `convert` 转换样本格式), 合成完成且 buffer 消费空时置
/// `playback_done`, 供 `speak_streaming` 通过 `is_done()` 退出等待.
fn drain_into<T>(
    buffer: &Arc<Mutex<Vec<f32>>>,
    synthesis_done: &Arc<AtomicBool>,
    playback_done: &Arc<AtomicBool>,
    data: &mut [T],
    convert: impl Fn(f32) -> T,
) {
    let mut buffer = buffer.lock().unwrap();
    let to_read = std::cmp::min(data.len(), buffer.len());
    for (i, sample) in data.iter_mut().enumerate() {
        let s = if i < to_read { buffer[i] } else { 0.0 };
        *sample = convert(s);
    }
    if to_read > 0 {
        buffer.drain(0..to_read);
    }
    // 合成完成 + buffer 空 = 播放完成
    if synthesis_done.load(Ordering::SeqCst) && buffer.is_empty() {
        playback_done.store(true, Ordering::SeqCst);
    }
}

/// 流式 TTS 播放句柄.
///
/// 持有一个共享 buffer (合成线程写入) + 一个 cpal `OutputStream` (回调线程读取).
/// 调用方**必须**持有本句柄到 `is_done()` 返回 `true` 再 Drop, Drop 链上的
/// `OwnedOutputStream` 才会停流并释放 device 句柄. 切音频设备时
/// `SharedState::rebuild_voice` 会通过 `Arc` drop 链触发本类型 Drop.
pub struct StreamPlayerHandle {
    /// 合成线程写入, 播放回调消费的音频 buffer
    buffer: Arc<Mutex<Vec<f32>>>,
    /// 合成完成标志 (合成线程 set)
    synthesis_done: Arc<AtomicBool>,
    /// 播放完成标志 (cpal 回调 set)
    playback_done: Arc<AtomicBool>,
    /// cpal `OutputStream` RAII 包装, Drop 时停流 + 释放 device 句柄
    _stream: OwnedOutputStream,
}

impl StreamPlayerHandle {
    /// 把一个音频块追加到共享 buffer. 线程安全, 可在合成线程里调.
    pub fn write_chunk(&self, chunk: &[f32], progress: f32) {
        if let Ok(mut buffer) = self.buffer.lock() {
            buffer.extend_from_slice(chunk);
            log::info!("deal progress: {progress}");
        }
    }

    /// 标记合成为完成. cpal 回调检测到 `synthesis_done && buffer 空` 时会
    /// 触发 `playback_done = true`, 调用方通过 `is_done()` 轮询得知.
    pub fn mark_synthesis_done(&self) {
        self.synthesis_done.store(true, Ordering::SeqCst);
    }

    /// 是否播放完成 (合成结束 + buffer 已消费完).
    #[must_use]
    pub fn is_done(&self) -> bool {
        self.playback_done.load(Ordering::SeqCst)
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
    /// 找不到设备时返回 `Err`. 设备句柄持有到 `TtsPlayer` Drop.
    ///
    /// # Examples
    /// ```rust,ignore
    /// let player = TtsPlayer::new("", None)?; // 默认设备
    /// let player = TtsPlayer::new("Speakers (Realtek)", None)?;
    /// let player = TtsPlayer::new("", Some("{0.0.0...}"))?; // 按 id 优先
    /// ```
    pub fn new(output_device_name: &str, output_device_id: Option<&str>) -> Result<Self> {
        let device = crate::media::voice::find_output_device(output_device_name, output_device_id)
            .ok_or_else(|| anyhow!("No audio output device found"))?;

        log::info!("TTS output device: {:?}", device.description());

        let sample_format = Self::detect_sample_format(&device);
        Ok(Self {
            device,
            sample_format,
        })
    }

    /// 试开流探测设备真实支持的采样格式: F32 优先, 开不动退 I16.
    ///
    /// 部分 Linux USB 声卡 (如 PCM2912A) 硬件只收 S16_LE, 且 cpal 的
    /// `supported_output_configs` 会虚报 F32 支持, 只能实际开流验证.
    /// 探测用的 channels/rate 与 TTS 实际播放一致 (mono/16k).
    fn detect_sample_format(device: &Device) -> cpal::SampleFormat {
        let config = cpal::StreamConfig {
            channels: 1,
            sample_rate: 16000,
            buffer_size: cpal::BufferSize::Default,
        };
        let f32_ok = device
            .build_output_stream(
                config,
                |_: &mut [f32], _: &cpal::OutputCallbackInfo| {},
                |_| {},
                None,
            )
            .is_ok();
        if f32_ok {
            cpal::SampleFormat::F32
        } else {
            log::info!("TTS device rejects f32 stream, will play as i16");
            cpal::SampleFormat::I16
        }
    }

    /// 按格式构造输出流: 运行时 `SampleFormat` -> 编译期样本类型的唯一点.
    ///
    /// cpal 的样本格式是编译期泛型, 分发无法彻底消除, 收在这一个方法里 —
    /// 新增格式只需在这加一个 match 分支. 回调以工厂闭包传入, 因为
    /// fallback 重试时要再造一份.
    fn build_stream(
        &self,
        config: cpal::StreamConfig,
        fmt: cpal::SampleFormat,
        make_f32_cb: &mut dyn FnMut() -> F32Callback,
        make_i16_cb: &mut dyn FnMut() -> I16Callback,
    ) -> std::result::Result<cpal::Stream, cpal::Error> {
        let err_fn = |err| log::error!("TTS 流错误: {err}");
        match fmt {
            cpal::SampleFormat::F32 => {
                self.device
                    .build_output_stream(config, make_f32_cb(), err_fn, None)
            }
            cpal::SampleFormat::I16 => {
                self.device
                    .build_output_stream(config, make_i16_cb(), err_fn, None)
            }
            // detect_sample_format 只会选出 F32/I16
            _ => unreachable!("TtsPlayer sample format is only F32/I16"),
        }
    }

    /// 按创建时探测的格式开流, 失败用另一种格式兜底一次 (防御播放参数的
    /// channels/rate 与探测不一致的极端情况).
    fn build_stream_with_fallback(
        &self,
        config: cpal::StreamConfig,
        mut make_f32_cb: impl FnMut() -> F32Callback,
        mut make_i16_cb: impl FnMut() -> I16Callback,
    ) -> std::result::Result<cpal::Stream, cpal::Error> {
        let (primary, fallback) = match self.sample_format {
            cpal::SampleFormat::I16 => (cpal::SampleFormat::I16, cpal::SampleFormat::F32),
            _ => (cpal::SampleFormat::F32, cpal::SampleFormat::I16),
        };
        match self.build_stream(config, primary, &mut make_f32_cb, &mut make_i16_cb) {
            Ok(s) => Ok(s),
            Err(e) => {
                log::warn!("{primary:?} output stream failed ({e}), fallback to {fallback:?}");
                self.build_stream(config, fallback, &mut make_f32_cb, &mut make_i16_cb)
            }
        }
    }

    /// 构造一个 cpal `OutputStream` 回调闭包, 把 `samples` 写到 cpal 推过来的
    /// 输出 buffer, 同时累计已写入样本数到 `played` (供 `play` 等播放完成用).
    fn write_audio_callback(
        samples: Vec<f32>,
        played: Arc<AtomicUsize>,
    ) -> impl FnMut(&mut [f32], &cpal::OutputCallbackInfo) + Send + 'static {
        let total = samples.len();

        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            let pos = played.load(Ordering::Relaxed);
            for (i, sample) in data.iter_mut().enumerate() {
                if pos + i < total {
                    *sample = samples[pos + i];
                } else {
                    *sample = 0.0;
                }
            }
            played.fetch_add(data.len(), Ordering::Relaxed);
        }
    }

    /// `write_audio_callback` 的 i16 变体 — 回调里把 f32 样本转成 S16.
    ///
    /// 部分 Linux USB 声卡 (如 PCM2912A) 硬件只收 S16_LE, 用 f32 开流会被
    /// cpal 拒绝 ("stream configuration is not supported"), 此时走这条路.
    fn write_audio_callback_i16(
        samples: Vec<f32>,
        played: Arc<AtomicUsize>,
    ) -> impl FnMut(&mut [i16], &cpal::OutputCallbackInfo) + Send + 'static {
        let total = samples.len();

        move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
            let pos = played.load(Ordering::Relaxed);
            for (i, sample) in data.iter_mut().enumerate() {
                let s = if pos + i < total {
                    samples[pos + i]
                } else {
                    0.0
                };
                *sample = (s * 32767.0).clamp(-32768.0, 32767.0) as i16;
            }
            played.fetch_add(data.len(), Ordering::Relaxed);
        }
    }

    /// 把 TTS 音频推完整个 buffer. 阻塞到 cpal 回调真正把 `audio.samples.len()`
    /// 个样本写过 `OutputStream` 才返回.
    ///
    /// 不再用 wall-clock `thread::sleep` 估算时长 — 设备采样率不匹配 / buffer
    /// 大小 / OS 调度抖动都会让 sleep 提前返回截断音频. 改用回调里累计的
    /// `Arc<AtomicUsize>` 作为完成信号.
    pub fn play(&self, audio: &TtsAudio) -> Result<()> {
        let total = audio.samples.len();
        let played = Arc::new(AtomicUsize::new(0));

        let config = cpal::StreamConfig {
            channels: audio.channels,
            sample_rate: audio.sample_rate,
            buffer_size: cpal::BufferSize::Default,
        };

        let mut make_f32_cb = {
            let (samples, played) = (audio.samples.clone(), played.clone());
            move || -> F32Callback {
                Box::new(Self::write_audio_callback(samples.clone(), played.clone()))
            }
        };
        let mut make_i16_cb = {
            let (samples, played) = (audio.samples.clone(), played.clone());
            move || -> I16Callback {
                Box::new(Self::write_audio_callback_i16(
                    samples.clone(),
                    played.clone(),
                ))
            }
        };
        let stream = self.build_stream_with_fallback(config, &mut make_f32_cb, &mut make_i16_cb)?;

        stream.play()?;

        // 阻塞到 callback 真正写完全部样本. 10ms 节流避免 busy-loop.
        // `stream` 在函数返回时自然 drop, cpal 停流并释放 device 句柄.
        while played.load(Ordering::Relaxed) < total {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        Ok(())
    }

    /// 启动流式播放. 返回的 `StreamPlayerHandle` 持有 cpal `OutputStream`,
    /// 调用方必须持有到 `is_done()` 返回 `true` 再 Drop. Drop 时
    /// `OwnedOutputStream` 停流并释放 device 句柄.
    ///
    /// 调用方模式 (`VoiceManager::speak_streaming`):
    /// 1. `let handle = player.start_streaming(...)?;`
    /// 2. `let handle = Arc::new(handle);` 给合成线程一份 clone
    /// 3. 合成线程 `handle.write_chunk(...)` + `handle.mark_synthesis_done()`
    /// 4. 调用方 `while !handle.is_done() { sleep(50ms) }`
    /// 5. `handle` 离开作用域 → `OwnedOutputStream::drop` → 设备释放
    ///
    /// # 边界
    /// - 设备正在被独占占用时会返回 `Err`
    /// - 切音频设备 (`SharedState::rebuild_voice`) 触发时, 仍在播放的句柄
    ///   要等 `is_done()` 后才被 drop — 这条延迟路径在本 change 范围内不解决
    ///   ("切设备立即打断 TTS" 是未来 change).
    ///
    /// # Examples
    /// ```rust,ignore
    /// let handle = player.start_streaming(22050)?;
    /// // ... 合成线程 write_chunk + mark_synthesis_done
    /// while !handle.is_done() { std::thread::sleep(...); }
    /// drop(handle); // 释放 cpal stream + device 句柄
    /// ```
    pub fn start_streaming(&self, sample_rate: u32) -> Result<StreamPlayerHandle> {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let synthesis_done = Arc::new(AtomicBool::new(false));
        let playback_done = Arc::new(AtomicBool::new(false));

        let buffer_clone = buffer.clone();
        let synthesis_done_clone = synthesis_done.clone();
        let playback_done_clone = playback_done.clone();

        let config = cpal::StreamConfig {
            channels: 1,
            sample_rate,
            buffer_size: cpal::BufferSize::Default,
        };

        // 构造 cpal OutputStream. callback 里从共享 buffer head drain 已合成
        // 的样本, 写进 cpal 推过来的输出 buffer. synthesis_done && buffer 空
        // 时设 playback_done, 让调用方通过 is_done() 退出.
        //
        // 采样格式用创建时 detect_sample_format 试开流探测的结果 (部分 USB
        // 声卡只收 S16_LE, 且 cpal 的 supported_output_configs 会虚报 F32),
        // 与 play() 同策略.
        let mut make_f32_cb = {
            let (b, s, p) = (
                buffer_clone.clone(),
                synthesis_done_clone.clone(),
                playback_done_clone.clone(),
            );
            move || -> F32Callback {
                let (b, s, p) = (b.clone(), s.clone(), p.clone());
                Box::new(move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    drain_into(&b, &s, &p, data, |s| s)
                })
            }
        };
        let mut make_i16_cb = {
            let (b, s, p) = (buffer_clone, synthesis_done_clone, playback_done_clone);
            move || -> I16Callback {
                let (b, s, p) = (b.clone(), s.clone(), p.clone());
                Box::new(move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    drain_into(&b, &s, &p, data, |s| {
                        (s * 32767.0).clamp(-32768.0, 32767.0) as i16
                    })
                })
            }
        };
        let stream = self.build_stream_with_fallback(config, &mut make_f32_cb, &mut make_i16_cb)?;

        stream.play()?;

        // Stream 跟着 handle 走, 禁止 mem::forget. 见 OwnedOutputStream 的 rustdoc.
        Ok(StreamPlayerHandle {
            buffer,
            synthesis_done,
            playback_done,
            _stream: OwnedOutputStream { stream },
        })
    }
}

impl Default for TtsPlayer {
    fn default() -> Self {
        // unwrap is safe here as it only fails if no audio device exists
        Self::new("", None).unwrap()
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
