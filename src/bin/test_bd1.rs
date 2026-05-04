// 测试 BD1 声音播放
// cargo run --bin test_bd1

use droid_chatter::{setup_sounds, AudioData, DroidChatter, Mood as DroidMood};
use rodio::cpal::traits::DeviceTrait;
use rodio::cpal::traits::HostTrait;
use std::num::NonZero;
use std::path::Path;
use std::thread;
use std::time::Duration;

fn main() -> anyhow::Result<()> {
    println!("=== BD1 Sound Test ===");

    let temp_dir = std::env::temp_dir();
    let sounds_dir = temp_dir.join("droid_sounds");
    let wav_path = Path::new("../../bd1_test.wav");

    // 下载/设置声音文件
    println!("Setting up sounds in {:?}", sounds_dir);
    setup_sounds(&sounds_dir)?;

    let chatter = DroidChatter::new(&sounds_dir)?;
    println!("DroidChatter created");

    // 获取音频数据
    let audio_data = chatter.bd1_audio("Hello", DroidMood::Happy)?;
    println!(
        "Audio data: {} samples, {} Hz, {} channels",
        audio_data.samples.len(),
        audio_data.sample_rate,
        audio_data.channels
    );

    // 保存 WAV
    write_wav(&audio_data, wav_path)?;
    println!("Saved WAV to {:?}", wav_path);

    // 播放
    println!("Playing via cpal...");
    #[cfg(target_os = "linux")]
    play_audio(&audio_data, "sysdefault:CARD=CODEC")?;
    #[cfg(target_os = "macos")]
    play_audio(&audio_data, "BuiltInSpeakerDevice")?;

    println!("Done!");
    Ok(())
}

fn write_wav(audio_data: &AudioData, path: &Path) -> anyhow::Result<()> {
    use hound::{SampleFormat, WavSpec, WavWriter};

    let spec = WavSpec {
        channels: audio_data.channels,
        sample_rate: audio_data.sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };

    let mut writer = WavWriter::create(path, spec)?;
    for &sample in &audio_data.samples {
        writer.write_sample(sample)?;
    }
    writer.finalize()?;
    Ok(())
}

/// 播放原始音频, 不同平台使用的设备名称不一样, 阻塞直到播放完成
///
/// # Arguments
///
/// * `audio_data`: 音频数据
/// * `device_name`: 播放的设备名称
///
/// returns: Result<(), Error>
///
/// # Examples
///
/// ```
/// #[cfg(target_os = "linux")]
/// play_audio(&audio_data, "sysdefault:CARD=CODEC")?;
/// #[cfg(target_os = "macos")]
/// play_audio(&audio_data, "BuiltInSpeakerDevice")?;
/// ```
fn play_audio(audio_data: &AudioData, device_name: &str) -> anyhow::Result<()> {
    let host = cpal::default_host();
    let mut devices = host.output_devices()?;

    let device = devices
        .find(|d| {
            d.id()
                .map(|n| {
                    // println!("name: {}", n.1);
                    n.1.contains(device_name)
                })
                .unwrap_or(false)
        })
        .ok_or_else(|| anyhow::anyhow!("Find: {} failed.", device_name))?;

    // rodio 0.22 的 Sample = f32，必须转换
    let samples_f32: Vec<f32> = audio_data
        .samples
        .iter()
        .map(|&s| s as f32 / i16::MAX as f32)
        .collect();

    // 创建 SamplesBuffer
    let buffer = rodio::buffer::SamplesBuffer::new(
        NonZero::new(audio_data.channels).unwrap(),
        NonZero::new(audio_data.sample_rate).unwrap(),
        samples_f32,
    );

    let sink = rodio::DeviceSinkBuilder::from_device(device)
        .map_err(|e| anyhow::anyhow!("Failed to create sink: {}", e))?
        .open_stream()?;
    let mixer = sink.mixer();
    mixer.add(buffer);

    // 计算播放时长并等待, 使用Buffer机制的没有内部阻塞, 只能手动计算了
    let total_samples = audio_data.samples.len() as f64 / audio_data.channels as f64;
    let duration_secs = total_samples / audio_data.sample_rate as f64;
    thread::sleep(Duration::from_secs_f64(duration_secs + 0.5));

    Ok(())
}
