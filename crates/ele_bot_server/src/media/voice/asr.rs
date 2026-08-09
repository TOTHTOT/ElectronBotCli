//! ASR 模块 - 麦克风输入流 + VAD + SenseVoice 离线识别
//!
//! 数据流:
//! ```text
//! cpal stream ──► process_audio_chunk() ──► audio_tx ──► recognition_loop()
//!                              │                                │
//!                              ▼                                ▼
//!                      bus.publish(Volume)        bus.publish(AsrText)
//!                              │                                │
//!                              ▼                                ▼
//!                  WS / WebPreview 客户端             LLM tokio task
//! ```
//!
//! `process_audio_chunk` 由 cpal callback 线程调用 (~30 Hz, 每 32 ms 一帧).
//! `recognition_loop` 在 std thread 跑, 通过 `audio_rx.recv_timeout(50ms)` 拉
//! 数据 + 检测 VAD, 触发时给 `OfflineRecognizer` 喂 buffer 解码, 命中后
//! `bus.publish(AsrText)` 让 LLM task 收到.
//!
//! `running` 标志是 `rebuild_voice` 的退出通道 — `VoiceManager` Drop 时
//! `store(false)`, 本 loop 在下次 `recv_timeout` 唤醒时立刻 return Ok, 不
//! 阻塞 cpal backend 停回调的 race.
#![allow(dead_code)]

use crate::media::voice::{AsrModelPaths, VAD_WINDOW_SIZE};
use cpal::traits::DeviceTrait;
use cpal::{Device, SampleRate, Stream};
use sherpa_onnx::{
    OfflineModelConfig, OfflineRecognizer, OfflineRecognizerConfig, OfflineSenseVoiceModelConfig,
    VadModelConfig, VoiceActivityDetector,
};
use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ── 采样 / 缓冲 / VAD 常量 ────────────────────────────────────────────────
const SAMPLE_RATE: usize = 16_000;
/// 说话触发前的静音缓冲, 用作识别 buffer 的 pre-context.
const PRE_ROLL_MS: usize = 500;
const PRE_ROLL_SAMPLES: usize = SAMPLE_RATE / 1000 * PRE_ROLL_MS;
/// 累计 ~31 帧静音 (~1 秒) 就把一截语音当成 "end of speech" 提交给 recognizer.
/// 对话机器人要的是响应快; VAD 自身还有 min_silence_duration=0.3s 防抖,
/// 1s 判停足够. 注意尾巴会算进识别 buffer, 阈值过大会稀释 rknn 固定窗口
/// (5s) 里有效语音的占比, 还徒增延迟.
const SILENCE_THRESHOLD: usize = 30;
/// 语音 buffer 小于这个 (0.5s) 不提交, 防止噪音抖动误触发.
const MIN_AUDIO_LEN: usize = SAMPLE_RATE / 2;
/// 说话时 buffer 上限 (30s), 防止过长录音占用 recognizer 内存.
const MAX_SPEECH_SECONDS: usize = 30;
const MAX_SPEECH_SAMPLES: usize = SAMPLE_RATE * MAX_SPEECH_SECONDS;
/// 静音时 buffer 保留窗口 (1.5s), 给下一段说话一个 pre-context.
const BUFFER_WINDOW_MS: usize = 1_500;
const BUFFER_WINDOW_SAMPLES: usize = SAMPLE_RATE / 1000 * BUFFER_WINDOW_MS;

// ── 音量 publish 节流 ──────────────────────────────────────────────────────
/// `process_audio_chunk` 每次 cpal callback (~30 Hz) 都想 publish `Volume`,
/// 限频到 ~10 Hz 让 broadcast channel 内部 ring buffer / receiver-count 不
/// 在高频 churn. 100ms 是 UI 感知阈值的下限.
const VOLUME_PUBLISH_MIN_INTERVAL_MS: u64 = 100;

/// 共享节流状态. cpal 进程全局只有一个 input stream, 单一 callback 线程,
/// 单一 `LAST_VOLUME_PUBLISH_MS` 足够; 多 stream 同时存在的扩展场景下会
/// 竞争写入 (relaxed 顺序), 但都把限频拉向保守侧, 不会更糟.
static LAST_VOLUME_PUBLISH_MS: AtomicU64 = AtomicU64::new(0);

// ── 模型初始化 ──────────────────────────────────────────────────────────────

/// Initialize `SenseVoice` recognizer using sherpa-onnx
fn init_sense_voice(model_path: &Path, tokens_path: &Path) -> anyhow::Result<OfflineRecognizer> {
    // 模型是 .rknn 时走 Rockchip NPU: provider 必须声明 "rknn",
    // 且 rknn sense-voice 不支持 ITN (官方示例均关闭).
    let is_rknn = model_path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("rknn"));
    let config = OfflineRecognizerConfig {
        model_config: OfflineModelConfig {
            sense_voice: OfflineSenseVoiceModelConfig {
                model: Some(model_path.to_string_lossy().to_string()),
                language: Some("auto".to_string()),
                use_itn: !is_rknn,
            },
            tokens: Some(tokens_path.to_string_lossy().to_string()),
            provider: is_rknn.then(|| "rknn".to_string()),
            num_threads: 1,
            ..Default::default()
        },
        ..Default::default()
    };
    log::info!(
        "SenseVoice recognizer: model={model_path:?}, provider={}",
        if is_rknn { "rknn (NPU)" } else { "cpu" }
    );

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

/// Speech recognition main loop: VAD detection + `SenseVoice` recognition
///
/// 用 `audio_rx.recv_timeout(50ms)` 替代阻塞 `recv`, 每轮检查
/// `running` 标志: 一旦为 false, 立即返回. 这是 `rebuild_voice`
/// 软替换旧实例的退出通道, 不依赖 cpal backend 及时停回调.
///
/// 识别结果通过 `bus: EventBus` publish (`BusEvent::AsrText`) 流向 LLM,
/// 不再用专用 channel (commit event-bus-refactor 改动).
fn recognition_loop(
    recognizer: &mut OfflineRecognizer,
    vad: &mut VoiceActivityDetector,
    audio_rx: Receiver<Vec<f32>>,
    bus: crate::event_bus::EventBus,
    running: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let mut buffer: VecDeque<f32> = VecDeque::with_capacity(BUFFER_WINDOW_SAMPLES);
    let mut speaking = false;
    let mut silence_count = 0;

    loop {
        let samples = match audio_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(s) => s,
            Err(RecvTimeoutError::Timeout) => {
                if !running.load(Ordering::Relaxed) {
                    log::info!("ASR receive exit flag");
                    return Ok(());
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        };

        vad.accept_waveform(&samples);
        let is_speech = vad.detected();

        let max_len = if speaking {
            MAX_SPEECH_SAMPLES
        } else {
            BUFFER_WINDOW_SAMPLES
        };
        while buffer.len() + samples.len() > max_len {
            buffer.pop_front();
        }
        buffer.extend(samples.iter().copied());

        if is_speech {
            if !speaking {
                log::info!(">>> Speech start");
                speaking = true;
            }
            silence_count = 0;
        } else if speaking {
            silence_count += 1;
            if silence_count > SILENCE_THRESHOLD {
                finalize_speech(
                    &mut buffer,
                    &mut speaking,
                    &mut silence_count,
                    vad,
                    recognizer,
                    &bus,
                );
            }
        }
    }
}

/// 一段语音识别结束: 把 buffer 给 recognizer, 命中结果 publish, 然后
/// 截短 buffer 到 BUFFER_WINDOW_SAMPLES (作为下一次说话的 pre-context),
/// 重置 speaking / silence_count / vad.clear. 解耦出独立函数让主 loop
/// 扁平, 调试也方便.
fn finalize_speech(
    buffer: &mut VecDeque<f32>,
    speaking: &mut bool,
    silence_count: &mut usize,
    vad: &mut VoiceActivityDetector,
    recognizer: &mut OfflineRecognizer,
    bus: &crate::event_bus::EventBus,
) {
    log::info!(
        "<<< Speech end, {:.2}s",
        buffer.len() as f32 / SAMPLE_RATE as f32
    );

    if buffer.len() > MIN_AUDIO_LEN {
        let audio: Vec<f32> = buffer.iter().copied().collect();
        let stream = recognizer.create_stream();
        stream.accept_waveform(SAMPLE_RATE as i32, &audio);
        recognizer.decode(&stream);

        if let Some(result) = stream.get_result() {
            let text = result.text.trim().to_string();
            if !text.is_empty() {
                log::info!("ASR: 【{text}】");
                bus.publish(crate::event_bus::BusEvent::AsrText(text));
            }
        }
    }

    // 保留最近 BUFFER_WINDOW_SAMPLES 个采样作为下一次说话的 pre-context.
    while buffer.len() > BUFFER_WINDOW_SAMPLES {
        buffer.pop_front();
    }

    *speaking = false;
    *silence_count = 0;
    vad.clear();
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

/// 对一帧 f32 样本施加采集增益并 clamp 到 [-1.0, 1.0].
///
/// 增益 > 1 时不 clamp 会削波, 既破坏 ASR 输入也让电平显示恒打满;
/// 增益 == 1 是绝大多数时间的常见路径, 直接跳过省一遍乘法.
fn apply_gain(data: &mut [f32], gain: f32) {
    if gain == 1.0 {
        return;
    }
    for s in data.iter_mut() {
        *s = (*s * gain).clamp(-1.0, 1.0);
    }
}

/// 计算一帧音频的峰值音量并推进音量条 (attack/decay), 然后 downmix
/// 到单声道推给识别线程.
///
/// 采集增益在进入本函数时立即施加 (`gain` 原子量, f32 bits), 之后的
/// 峰值电平与 ASR 输入都是增益后信号 — 用户调麦克风音量立刻能在
/// 电平条上看到效果.
///
/// 音量条更新: 新峰值立即拉升, 否则按 0.95/帧 指数衰减 (半衰期约 0.4s).
/// 多声道按左右声道算术均值降混; 单声道直通.
/// 函数不获取 `volume` / `audio_tx` 的所有权, 由调用方持有 (通常是
/// cpal 输入流闭包, 闭包负责把变量 move 进来).
fn process_audio_chunk(
    data: &mut [f32],
    channels: usize,
    gain: &Arc<AtomicU32>,
    volume: &Arc<AtomicI32>,
    audio_tx: &SyncSender<Vec<f32>>,
    bus: &crate::event_bus::EventBus,
) {
    apply_gain(data, f32::from_bits(gain.load(Ordering::Relaxed)));

    let peak = data.iter().fold(0.0f32, |acc, &s| acc.max(s.abs()));
    let peak_value = peak_to_volume(peak);
    let current = volume.load(Ordering::Relaxed);
    let new_value = if peak_value > current {
        peak_value
    } else if current > 0 {
        ((current as f32) * 0.95) as i32
    } else {
        0
    };
    volume.store(new_value, Ordering::Relaxed);

    // 音量通过 EventBus 广播给 WS 客户端. 节流见模块顶部常量 + helper.
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let last_ms = LAST_VOLUME_PUBLISH_MS.load(Ordering::Relaxed);
    if should_publish_volume(current, new_value, last_ms, now_ms) {
        bus.publish(crate::event_bus::BusEvent::Volume(new_value));
        LAST_VOLUME_PUBLISH_MS.store(now_ms, Ordering::Relaxed);
    }

    // 音频数据
    let mono: Vec<f32> = if channels == 2 {
        data.chunks(2).map(|c| f32::midpoint(c[0], c[1])).collect()
    } else {
        data.to_vec()
    };
    let _ = audio_tx.send(mono);
}

/// 音量 publish 节流决策. 抽出函数后单测可以直接覆盖各种边角.
///
/// 抑制规则:
/// - `new_value == 0` 不发 (衰减到底, 没新信息)
/// - `new_value > current` 立即发 (峰值上升要立刻反馈)
/// - 余下路径间隔 ≥ [`VOLUME_PUBLISH_MIN_INTERVAL_MS`] ms
///
/// `last_ms` 是上次 publish 的 unix epoch 毫秒; 第一次调用传 0.
fn should_publish_volume(current: i32, new_value: i32, last_ms: u64, now_ms: u64) -> bool {
    if new_value <= 0 {
        return false;
    }
    if new_value > current {
        return true;
    }
    now_ms.saturating_sub(last_ms) >= VOLUME_PUBLISH_MIN_INTERVAL_MS
}

/// Build audio input stream for ASR
///
/// 按设备 `default_input_config` 上报的采样格式建流, 回调内统一转 f32
/// 交给 `process_audio_chunk`. 硬编码 f32 会在仅支持 S16 的硬件
/// (USB 声卡的 `front:` / `hw:` PCM) 上被 cpal 直接拒绝:
/// "Sample format 'f32' is not supported by hardware".
pub fn build_asr_stream(
    device: &Device,
    gain: Arc<AtomicU32>,
    volume: Arc<AtomicI32>,
    audio_tx: SyncSender<Vec<f32>>,
    bus: crate::event_bus::EventBus,
) -> anyhow::Result<Stream> {
    let config = device.default_input_config()?;
    log::info!("Audio device: {config:?}");

    let stream_config = cpal::StreamConfig {
        channels: config.channels(),
        sample_rate: SAMPLE_RATE as SampleRate,
        buffer_size: cpal::BufferSize::Fixed(512),
    };
    let channels = config.channels() as usize;

    match config.sample_format() {
        cpal::SampleFormat::I16 => {
            build_typed_stream::<i16>(device, stream_config, channels, gain, volume, audio_tx, bus)
        }
        cpal::SampleFormat::U16 => {
            build_typed_stream::<u16>(device, stream_config, channels, gain, volume, audio_tx, bus)
        }
        cpal::SampleFormat::I32 => {
            build_typed_stream::<i32>(device, stream_config, channels, gain, volume, audio_tx, bus)
        }
        cpal::SampleFormat::F32 => {
            build_typed_stream::<f32>(device, stream_config, channels, gain, volume, audio_tx, bus)
        }
        format => anyhow::bail!("Unsupported input sample format: {format}"),
    }
}

/// 按具体采样类型建输入流, 回调里转成 f32 再走统一的音量/ASR 处理.
fn build_typed_stream<T>(
    device: &Device,
    stream_config: cpal::StreamConfig,
    channels: usize,
    gain: Arc<AtomicU32>,
    volume: Arc<AtomicI32>,
    audio_tx: SyncSender<Vec<f32>>,
    bus: crate::event_bus::EventBus,
) -> anyhow::Result<Stream>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    // 根据设置的stream_config申请音频流
    Ok(device.build_input_stream(
        stream_config,
        move |data: &[T], _: &_| {
            let mut f32_data: Vec<f32> = data.iter().map(|s| s.to_sample::<f32>()).collect();
            process_audio_chunk(&mut f32_data, channels, &gain, &volume, &audio_tx, &bus);
        },
        |e| log::error!("Audio stream error: {e}"),
        None,
    )?)
}

/// Speech recognition thread entry
pub fn recognition_thread(
    model_path: AsrModelPaths,
    audio_rx: Receiver<Vec<f32>>,
    bus: crate::event_bus::EventBus,
    running: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let mut recognizer = init_sense_voice(&model_path.sense_voice, &model_path.tokens)?;
    let mut vad = init_silero_vad(&model_path.silero_vad)?;
    recognition_loop(&mut recognizer, &mut vad, audio_rx, bus, running)
}

/// Use `cargo test test_recognition_with_audio_file -- --ignored --nocapture` to test
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_manager::ModelManager;
    use std::fs::File;
    use std::io::Read;
    use std::path::{Path, PathBuf};

    /// 仓库根目录, 编译期从 crate manifest 目录往上两级.
    ///
    /// `cargo test` 的 cwd 不一定是仓库根, 用相对路径 `assets/audio/...`
    /// 会找不到文件. `CARGO_MANIFEST_DIR` 在编译时被 cargo 替换为 crate
    /// 目录绝对路径 (`crates/ele_bot_server`), 拼 `../..` 回到仓库根.
    /// 这样无论从哪个目录跑 `cargo test` 都能定位到 wav.
    const WORKSPACE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    const TEST_WAV_PATH: &str = "assets/audio/asr_example_zh.wav";

    fn test_wav_path() -> PathBuf {
        Path::new(WORKSPACE_ROOT).join(TEST_WAV_PATH)
    }

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

    /// 找到样本中能量首次超过 dBFS 阈值的样本序号.
    ///
    /// 滑动窗口 16ms (256 样本), 计算窗口内 RMS, 第一个 ≥ 阈值的窗口
    /// 起点即为"真实语音起点". 用于对照 `vad.detected()` 的样本位置,
    /// 算出触发延迟. 仅做测量用, 不参与产品逻辑.
    fn first_speech_sample(samples: &[f32], threshold_dbfs: f32) -> usize {
        const WINDOW: usize = 256; // 16ms @ 16kHz
        if samples.len() < WINDOW {
            return samples.len();
        }
        let threshold_rms = 10f32.powf(threshold_dbfs / 20.0);
        let mut sum_sq = 0.0f32;
        for &s in &samples[..WINDOW] {
            sum_sq += s * s;
        }
        let mut rms = (sum_sq / WINDOW as f32).sqrt();
        if rms >= threshold_rms {
            return 0;
        }
        for i in WINDOW..samples.len() {
            let old = samples[i - WINDOW];
            let new = samples[i];
            sum_sq = sum_sq - old * old + new * new;
            rms = (sum_sq / WINDOW as f32).sqrt();
            if rms >= threshold_rms {
                return i - WINDOW + 1;
            }
        }
        samples.len()
    }

    /// 计算两个字符串按 `char` (而不是 byte) 切片的最长公共前缀长度.
    ///
    /// 中文 UTF-8 是变长字节, 直接用 `str::as_bytes()` 切到字中间会
    /// panic. 按 `char` zip 后取共同前缀, 才能正确数"几个字".
    fn longest_common_prefix_chars(a: &str, b: &str) -> usize {
        a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count()
    }

    #[test]
    fn test_load_wav_samples() {
        // 锚到 workspace 根: cargo test 的 cwd 是 crate 目录, 相对路径会落空
        let samples = load_wav_samples(Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/audio/asr_example_zh.wav"
        )))
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
    fn apply_gain_scales_and_clamps() {
        // 单位增益: 原样 (常见路径)
        let mut d = vec![0.5, -0.5];
        apply_gain(&mut d, 1.0);
        assert_eq!(d, vec![0.5, -0.5]);

        // 半增益: 峰值减半 (调低麦克风音量的效果)
        let mut d = vec![0.4, -0.2];
        apply_gain(&mut d, 0.5);
        assert_eq!(d, vec![0.2, -0.1]);

        // 静音: 全零
        let mut d = vec![0.4, -0.2];
        apply_gain(&mut d, 0.0);
        assert_eq!(d, vec![0.0, 0.0]);

        // 过增益: clamp 不削波越界
        let mut d = vec![0.8, -0.6];
        apply_gain(&mut d, 2.0);
        assert_eq!(d, vec![1.0, -1.0]);
    }

    #[test]
    fn should_publish_volume_throttling() {
        // 衰减到底: 不发
        assert!(!should_publish_volume(50, 0, 0, 1000));
        // 峰值上升: 即使刚刚发过也立即发
        assert!(should_publish_volume(50, 80, 900, 950));
        // 平路 + 时间间隔够 100ms: 发
        assert!(should_publish_volume(50, 50, 0, 150));
        // 平路 + 间隔不够 100ms: 不发
        assert!(!should_publish_volume(50, 50, 0, 50));
        // 首次调用 (last_ms=0) + now_ms 任意大: 发 (saturating_sub 大于阈值)
        assert!(should_publish_volume(0, 10, 0, 1_000_000));
    }

    /// 把 `samples` 喂给 `recognition_loop` 跑完整识别, 返回 bus 收集到的
    /// 全部 `AsrText`. 喂数据在独立线程 (audio_tx 端), 识别在主线程
    /// (recognizer/vad 不是 Send); 尾部补 150 帧静音让 VAD 判停.
    fn run_recognition_on_samples(samples: Vec<f32>) -> Vec<String> {
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

        let (audio_tx, audio_rx) = std::sync::mpsc::sync_channel::<Vec<f32>>(4);

        let handle = std::thread::spawn(move || {
            let chunk_size = 1600;
            for chunk in samples.chunks(chunk_size) {
                audio_tx.send(chunk.to_vec()).expect("send failed");
            }
            // 静音帧给 VAD 时间检测 end of speech
            for _ in 0..150 {
                audio_tx
                    .send(vec![0.0f32; chunk_size])
                    .expect("send failed");
            }
            drop(audio_tx);
        });

        // 识别结果经 EventBus 流出, 测试通过订阅 bus 收集.
        let bus = crate::event_bus::EventBus::new(64);
        let mut bus_rx = bus.subscribe();
        let running = Arc::new(AtomicBool::new(true));
        let _ = recognition_loop(&mut recognizer, &mut vad, audio_rx, bus, running);
        handle.join().expect("thread panicked");

        let mut results: Vec<String> = Vec::new();
        while let Ok(evt) = bus_rx.try_recv() {
            if let crate::event_bus::BusEvent::AsrText(t) = evt {
                results.push(t);
            }
        }
        results
    }

    #[test]
    #[ignore]
    fn test_recognition_with_audio_file() {
        let samples = load_wav_samples(&test_wav_path()).expect("failed to load wav");
        let results = run_recognition_on_samples(samples);
        assert!(!results.is_empty(), "no recognition results");
        println!("Recognition results: {:?}", results);
    }

    /// 端到端跑 recognition_loop, 验证识别 asr_example_zh.wav 必须命中
    /// 19 字预期文本. 此前实测仅命中 14 字 (丢前 5 字), 现已修复到 19 字.
    /// 跑法: `cargo test --package ele_bot_server test_recognition_no_lost_chars -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn test_recognition_no_lost_chars() {
        let samples = load_wav_samples(&test_wav_path()).expect("failed to load wav");
        let expected = "欢迎大家来体验达摩院推出的语音识别模型";

        let recognized: String = run_recognition_on_samples(samples).join("");
        let common = longest_common_prefix_chars(&recognized, expected);

        println!("===== test_recognition_no_lost_chars =====");
        println!("识别结果: {:?}", recognized);
        println!("预期文本: {:?}", expected);
        println!(
            "共同前缀: {:?} ({} 字)",
            &expected.chars().take(common).collect::<String>(),
            common
        );

        // 硬断言: 必须命中完整 19 字. 若此 fail, 说明 recognition_loop
        // 又丢首字了, 需要回查修复.
        assert!(
            common >= 19,
            "识别仅 {} 字命中, 期望 ≥19. 完整识别: {:?}",
            common,
            recognized
        );
    }

    /// 找 wav 真实语音起点 (按 -30 dBFS RMS 阈值) + 逐帧喂 VAD,
    /// 打印 VAD 触发延迟 (ms). 用于诊断 VAD 配置是否合理.
    /// 跑法: `cargo test --package ele_bot_server test_vad_trigger_latency -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn test_vad_trigger_latency() {
        let samples = load_wav_samples(&test_wav_path()).expect("failed to load wav");

        let n_real = first_speech_sample(&samples, -30.0);

        let vad_path = ModelManager::global()
            .get("silero_vad")
            .expect("silero_vad model not found");
        let vad = init_silero_vad(&vad_path).expect("Failed to create VAD");

        const FRAME: usize = 512;
        let mut n_vad: Option<usize> = None;
        for (i, chunk) in samples.chunks(FRAME).enumerate() {
            vad.accept_waveform(chunk);
            if vad.detected() && n_vad.is_none() {
                n_vad = Some(i * FRAME);
                break;
            }
        }
        let n_vad = n_vad.expect("VAD 整段未触发, 阈值或模型配置异常");

        let real_ms = n_real as f32 / 16.0;
        let vad_ms = n_vad as f32 / 16.0;
        let latency_ms = vad_ms - real_ms;

        println!("===== test_vad_trigger_latency =====");
        println!("真实语音起点: 样本 {} ({:.0} ms)", n_real, real_ms);
        println!("VAD 触发点:   样本 {} ({:.0} ms)", n_vad, vad_ms);
        println!(
            "触发延迟:     {:.0} ms (≈ {} 个字 @250ms/字)",
            latency_ms,
            latency_ms / 250.0
        );
    }

    /// 模拟 recognition_loop 的 pre_roll 滑动窗口, 在 VAD 触发那一刻量
    /// 实际前文捕获量. 用于确认 pre_roll 容量充足.
    /// 跑法: `cargo test --package ele_bot_server test_pre_roll_capture_rate -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn test_pre_roll_capture_rate() {
        let samples = load_wav_samples(&test_wav_path()).expect("failed to load wav");

        let n_real = first_speech_sample(&samples, -30.0);

        let vad_path = ModelManager::global()
            .get("silero_vad")
            .expect("silero_vad model not found");
        let vad = init_silero_vad(&vad_path).expect("Failed to create VAD");
        const FRAME: usize = 512;
        let mut n_vad: Option<usize> = None;
        for (i, chunk) in samples.chunks(FRAME).enumerate() {
            vad.accept_waveform(chunk);
            if vad.detected() && n_vad.is_none() {
                n_vad = Some(i * FRAME);
                break;
            }
        }
        let n_vad = n_vad.expect("VAD 整段未触发");

        // 标准环形滑动窗口仿真
        const PRE_ROLL_SAMPLES: usize = 16000 / 1000 * 500;
        let cap = PRE_ROLL_SAMPLES.min(samples.len());
        let end = n_vad.min(samples.len());
        let start = end.saturating_sub(cap);
        let captured_samples = end - start;
        let captured_ms = captured_samples as f32 / 16.0;

        let pre_real_samples = if n_vad > n_real {
            (n_vad - n_real).min(PRE_ROLL_SAMPLES)
        } else {
            0
        };
        let pre_real_ms = pre_real_samples as f32 / 16.0;

        println!("===== test_pre_roll_capture_rate =====");
        println!(
            "pre_roll 容量: {} 样本 ({:.0} ms, 理论值)",
            PRE_ROLL_SAMPLES,
            PRE_ROLL_SAMPLES as f32 / 16.0
        );
        println!(
            "pre_roll 实际填到: {} 样本 ({:.0} ms)",
            captured_samples, captured_ms
        );
        println!(
            "其中真实语音前文: {} 样本 ({:.0} ms, VAD 触发距真实起点)",
            pre_real_samples, pre_real_ms
        );
        println!(
            "差值 (理论 - 实际): {:.0} ms",
            PRE_ROLL_SAMPLES as f32 / 16.0 - captured_ms
        );
    }
}
