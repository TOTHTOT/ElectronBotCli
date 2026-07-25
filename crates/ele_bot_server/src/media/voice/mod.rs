pub mod asr;
pub mod tts;

use crate::media::voice::asr::{build_asr_stream, recognition_thread};
use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Stream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{mpsc, Arc};

/// 从 cpal `Device` 同时取稳定 id 和 friendly name; 任一步失败返回 `None`.
///
/// 用于 `list_*_devices` 与 `find_*_device` 共享的"开箱"逻辑, 避免在 4 个
/// 调用点重复 `d.id().ok()?.to_string()` + `d.description().ok()?.name()` 模板.
fn device_id_and_name(d: &Device) -> Option<(String, String)> {
    let id = d.id().ok()?.to_string();
    let name = d.description().ok()?.name().to_string();
    Some((id, name))
}
use std::thread;

use self::tts::{TtsHandler, TtsPlayer};

pub const VAD_WINDOW_SIZE: i32 = 512;
#[allow(dead_code)]
pub const CHUNK_SIZE: usize = 1600; // 100ms at 16kHz
#[allow(dead_code)]
pub const SAMPLE_RATE: u32 = 16000;

/// ASR 模型路径集合 (sense_voice + silero_vad + tokens)
///
/// 三者必须同时存在才能跑识别, 打包在一起避免漏传.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct AsrModelPaths {
    pub sense_voice: PathBuf,
    pub silero_vad: PathBuf,
    pub tokens: PathBuf,
}

impl AsrModelPaths {
    pub fn new(
        sense_voice: impl AsRef<Path>,
        silero_vad: impl AsRef<Path>,
        tokens: impl AsRef<Path>,
    ) -> Self {
        Self {
            sense_voice: sense_voice.as_ref().into(),
            silero_vad: silero_vad.as_ref().into(),
            tokens: tokens.as_ref().into(),
        }
    }
}

/// TTS 模型路径集合 (vits 模型 + tokens + lexicon)
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TtsModelPaths {
    pub model: PathBuf,
    pub tokens: PathBuf,
    pub lexicon: PathBuf,
}

impl TtsModelPaths {
    pub fn new(
        model: impl AsRef<Path>,
        tokens: impl AsRef<Path>,
        lexicon: impl AsRef<Path>,
    ) -> Self {
        Self {
            model: model.as_ref().into(),
            tokens: tokens.as_ref().into(),
            lexicon: lexicon.as_ref().into(),
        }
    }
}

#[allow(dead_code)]
pub struct VoiceManager {
    _stream: Option<Stream>,
    volume: Arc<AtomicI32>,
    tts_handler: TtsHandler,
    tts_player: Option<TtsPlayer>,
    /// ASR 线程取消信号. 初始 true; `rebuild_voice` 重建前把旧实例
    /// 的 `running` 置 false, 旧 ASR 线程在 `audio_rx.recv_timeout`
    /// 唤醒时检查并主动退出.
    running: Arc<AtomicBool>,
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
    /// 完成后通过 EventBus 发出 ASR 识别文本
    ///
    /// `speech_device_id` / `output_device_id` 是 cpal `DeviceId` 序列化的
    /// 稳定标识, 与 `speech_name` / `output_device_name` 配套传入; 任一为
    /// `None` 或空时 `find_*_device` 自动按 name 兜底.
    pub fn new(
        asr_paths: AsrModelPaths,
        tts_paths: TtsModelPaths,
        speech_name: &str,
        speech_device_id: Option<&str>,
        output_device_name: &str,
        output_device_id: Option<&str>,
        bus: crate::event_bus::EventBus,
    ) -> Result<Self> {
        // 初始化 TTS
        let tts_handler = TtsHandler::new(&tts_paths.model, &tts_paths.tokens, &tts_paths.lexicon)?;
        let tts_player = Some(TtsPlayer::new(output_device_name, output_device_id)?);

        let volume = Arc::new(AtomicI32::new(0)); // 实时音量
        let running = Arc::new(AtomicBool::new(true));
        let (audio_tx, audio_rx) = mpsc::sync_channel::<Vec<f32>>(4); // 原始音频数据传输通道

        // 查找输入麦克风, 当设备不存在时也会继续执行, 只是不会运行到 asr 相关功能
        let stream = match find_input_device(speech_name, speech_device_id) {
            Ok(device) => {
                let stream = build_asr_stream(&device, volume.clone(), audio_tx, bus.clone())?;
                stream.play()?;
                Some(stream)
            }
            Err(e) => {
                log::warn!("Cannot find input device: {}", e);
                None
            }
        };

        // 创建解析音频线程, 识别结果经 EventBus 流向 LLM (不再有专用 text channel).
        let running_for_thread = running.clone();
        thread::spawn(move || {
            if let Err(e) = recognition_thread(
                asr_paths.sense_voice,
                asr_paths.silero_vad,
                asr_paths.tokens,
                audio_rx,
                bus,
                running_for_thread,
            ) {
                log::error!("recognition_thread failed: {e:?}");
            }
        });

        Ok(Self {
            _stream: stream,
            volume,
            tts_handler,
            tts_player,
            running,
        })
    }

    /// 获取实时音量
    pub fn volume(&self) -> i32 {
        self.volume.load(Ordering::Relaxed)
    }

    /// 返回 ASR 线程的取消信号.
    ///
    /// 外部 (如 `SharedState::rebuild_voice`) 调用 `store(false)` 请求
    /// 当前 ASR 线程主动退出. 线程在 `audio_rx.recv_timeout` 唤醒时
    /// 检查该标志, 50ms 内返回.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let old = state.voice.lock().unwrap().take();
    /// if let Some(v) = old {
    ///     v.running().store(false, Ordering::Relaxed);
    /// }
    /// ```
    pub fn running(&self) -> Arc<AtomicBool> {
        self.running.clone()
    }

    /// ASR 线程是否仍在跑 (running 标志当前值).
    #[allow(dead_code)]
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
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

/// 按稳定 ID 查找输入设备, ID 失效或未提供时按 name 兜底, 都没命中则回退默认.
///
/// 匹配顺序: cpal `DeviceId` 优先 (Windows: IMMDevice endpoint ID 字符串),
/// name 仅作为旧 config 或 ID 失效时的兜底. 全部为空或全部失败时回退到
/// `host.default_input_device()`.
fn find_input_device(speech_name: &str, device_id: Option<&str>) -> Result<Device> {
    let host = cpal::default_host();
    let devices: Vec<_> = host
        .input_devices()?
        .filter_map(|d| {
            let (id, name) = device_id_and_name(&d)?;
            Some((id, name, d))
        })
        .collect();
    log::debug!(
        "target: name={speech_name:?}, id={device_id:?}, input audio devices: {:?}",
        devices
            .iter()
            .map(|(id, name, _)| (id.as_str(), name.as_str()))
            .collect::<Vec<_>>()
    );

    let no_id = device_id.map(str::is_empty).unwrap_or(true);
    if speech_name.is_empty() && no_id {
        return host
            .default_input_device()
            .ok_or_else(|| anyhow!("No default audio input device found"));
    }

    //  优先按 cpal DeviceId 匹配
    if let Some(want_id) = device_id.filter(|s| !s.is_empty()) {
        if let Some((_, _, d)) = devices.iter().find(|(id, _, _)| id == want_id) {
            log::info!(
                "Matched input device by id: {want_id}, name {:?}, driver: {:?}",
                d.description()?.name(),
                d.description()?.driver()
            );
            let device = d.clone();
            if let Ok(config) = device.default_input_config() {
                log::info!("Selected audio device config: {config:?}");
            }
            return Ok(device);
        }
        log::warn!("Input device id '{want_id}' not found, falling back to name match");
    }

    // 兜底按 name 匹配
    if !speech_name.is_empty() {
        if let Some((_, _, d)) = devices.iter().find(|(_, n, _)| n == speech_name) {
            log::info!("Matched input device by name: {speech_name}");
            let device = d.clone();
            if let Ok(config) = device.default_input_config() {
                log::info!("Selected audio device config: {config:?}");
            }
            return Ok(device);
        }
    }

    anyhow::bail!("No audio input device found: name={speech_name:?}, id={device_id:?}");
}

/// 设备信息 - 用于在设置页面中显示并区分同名设备
///
/// 内部类型, 不直接序列化. 通过 [`list_input_devices_dto`] /
/// [`list_output_devices_dto`] 转 [`ele_bot_proto::DeviceInfoDto`] 后走 WS.
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// cpal 稳定设备标识 (Windows: IMMDevice endpoint ID 字符串,
    /// Linux: ALSA path, macOS: UID). 用于服务端按设备精确匹配.
    pub id: String,
    /// 实际设备名称 (cpal exact name). 仅作 `id` 失效时的兜底匹配键.
    pub name: String,
    /// 列表中显示的字符串, 包含通道数 / 采样率等额外信息
    pub display: String,
    /// 后端驱动名 (cpal Device::description().driver()), 不可用为 None
    pub driver: Option<String>,
    /// 输入/输出通道数, default_input_config 失败时为 None
    pub channels: Option<u16>,
    /// 默认采样率, default_input_config 失败时为 None
    pub sample_rate: Option<u32>,
}

impl DeviceInfo {
    fn new(
        id: String,
        name: String,
        channels: Option<u16>,
        sample_rate: Option<u32>,
        driver: Option<String>,
    ) -> Self {
        let mut parts: Vec<String> = Vec::new();
        if let Some(driver) = driver.as_ref() {
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
        Self {
            id,
            name,
            display,
            driver,
            channels,
            sample_rate,
        }
    }
}

/// 枚举系统所有输入设备, 包含通道数和采样率等额外信息
pub fn list_input_devices() -> Vec<DeviceInfo> {
    let host = cpal::default_host();
    host.input_devices()
        .map(|it| {
            it.filter_map(|d| {
                let (id, name) = device_id_and_name(&d)?;
                let driver = d
                    .description()
                    .ok()
                    .and_then(|x| x.driver().map(str::to_string));
                let (channels, sample_rate) = d
                    .default_input_config()
                    .map(|c| (Some(c.channels()), Some(c.sample_rate())))
                    .unwrap_or((None, None));
                Some(DeviceInfo::new(id, name, channels, sample_rate, driver))
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
            it.filter_map(|d| {
                let (id, name) = device_id_and_name(&d)?;
                let driver = d
                    .description()
                    .ok()
                    .and_then(|x| x.driver().map(str::to_string));
                let (channels, sample_rate) = d
                    .default_output_config()
                    .map(|c| (Some(c.channels()), Some(c.sample_rate())))
                    .unwrap_or((None, None));
                Some(DeviceInfo::new(id, name, channels, sample_rate, driver))
            })
            .collect()
        })
        .unwrap_or_default()
}

/// 把 [`DeviceInfo`] 转成 [`ele_bot_proto::DeviceInfoDto`] 给 ws 走线协议.
///
/// `channels` / `sample_rate` 为 None 时填 0, 与 wire 上 "u16/u32 默认值"
/// 保持一致; 客户端通过 `display` 是否包含这些信息自行判断真实可用性.
fn to_dto(info: &DeviceInfo) -> ele_bot_proto::DeviceInfoDto {
    ele_bot_proto::DeviceInfoDto {
        id: info.id.clone(),
        name: info.name.clone(),
        display: info.display.clone(),
        driver: info.driver.clone(),
        channels: info.channels.unwrap_or(0),
        sample_rate: info.sample_rate.unwrap_or(0),
    }
}

/// 把 [`list_input_devices`] 的结果序列化为 wire DTO, 给
/// `ServerEvent::InputDevices` 直接消费.
///
/// # Examples
///
/// ```rust,ignore
/// // 在 ws.rs::handle_command 里:
/// let devices = voice::list_input_devices_dto();
/// out_tx.send(ServerEvent::InputDevices { devices })?;
/// ```
pub fn list_input_devices_dto() -> Vec<ele_bot_proto::DeviceInfoDto> {
    list_input_devices().iter().map(to_dto).collect()
}

/// 把 [`list_output_devices`] 的结果序列化为 wire DTO. 同上, 给
/// `ServerEvent::OutputDevices` 用.
pub fn list_output_devices_dto() -> Vec<ele_bot_proto::DeviceInfoDto> {
    list_output_devices().iter().map(to_dto).collect()
}

/// 按稳定 ID 查找输出设备, ID 失效或未提供时按 name 兜底, 都没命中则回退默认.
///
/// 匹配顺序: cpal `DeviceId` 优先 (Windows: IMMDevice endpoint ID 字符串),
/// name 仅作为旧 config 或 ID 失效时的兜底. 全部为空或全部失败时回退到
/// `host.default_output_device()`.
pub fn find_output_device(name: &str, device_id: Option<&str>) -> Option<Device> {
    let host = cpal::default_host();
    let devices: Vec<_> = host
        .output_devices()
        .ok()
        .map(|it| {
            it.filter_map(|d| {
                let (id, name) = device_id_and_name(&d)?;
                Some((id, name, d))
            })
            .collect()
        })
        .unwrap_or_default();
    log::debug!(
        "target: name={name:?}, id={device_id:?}, output audio devices: {:?}",
        devices
            .iter()
            .map(|(id, name, _)| (id.as_str(), name.as_str()))
            .collect::<Vec<_>>()
    );

    let no_id = device_id.map(str::is_empty).unwrap_or(true);
    if name.is_empty() && no_id {
        return host.default_output_device();
    }

    // 1. 优先按 cpal DeviceId 匹配
    if let Some(want_id) = device_id.filter(|s| !s.is_empty()) {
        if let Some((_, _, d)) = devices.iter().find(|(id, _, _)| id == want_id) {
            log::info!("Matched output device by id: {want_id}");
            return Some(d.clone());
        }
        log::warn!("Output device id '{want_id}' not found, falling back to name match");
    }

    // 2. 兜底按 name 匹配
    if !name.is_empty() {
        if let Some((_, _, d)) = devices.iter().find(|(_, n, _)| n == name) {
            log::info!("Matched output device by name: {name}");
            return Some(d.clone());
        }
        log::warn!("Output device name '{name}' not found, falling back to default");
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
    output_device_id: Option<&str>,
) {
    let device = match find_output_device(output_device_name, output_device_id) {
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
