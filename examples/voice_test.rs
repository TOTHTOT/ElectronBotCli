use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rodio::{OutputStream, Sink};
use sherpa_rs::sense_voice::{SenseVoiceConfig, SenseVoiceRecognizer};
use sherpa_rs::tts::{TtsAudio, VitsTts, VitsTtsConfig};
use std::sync::mpsc;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        println!("Usage:");
        println!("  cargo run --example voice_test -- tts <text>  - Test text-to-speech");
        println!("  cargo run --example voice_test -- asr          - Test speech recognition");
        return;
    }

    match args[1].as_str() {
        "tts" => {
            let text = args[2..].join(" ");
            if let Err(e) = test_tts(&text) {
                eprintln!("TTS error: {:?}", e);
            }
        }
        "asr" => {
            if let Err(e) = test_asr() {
                eprintln!("ASR error: {:?}", e);
            }
        }
        _ => {
            println!("Unknown command: {}", args[1]);
        }
    }
}

fn test_tts(text: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("Testing TTS with text: {}", text);

    let config = VitsTtsConfig {
        model: "./assets/module/vits/model.onnx".into(),
        tokens: "./assets/module/vits/tokens.txt".into(),
        lexicon: "./assets/module/vits/lexicon.txt".into(),
        ..Default::default()
    };

    let mut tts = VitsTts::new(config);
    println!("TTS initialized successfully");

    // Generate audio
    let audio: TtsAudio = tts.create(text, 0, 1.0)?;
    println!(
        "Generated audio: {} samples, {} Hz, duration: {:.2}s",
        audio.samples.len(),
        audio.sample_rate,
        audio.duration
    );

    // Play audio
    let (_stream, stream_handle) = OutputStream::try_default()?;
    let sink = Sink::try_new(&stream_handle)?;

    let samples: Vec<f32> = audio
        .samples
        .iter()
        .map(|&s| s as f32 / i16::MAX as f32)
        .collect();

    let source = rodio::buffer::SamplesBuffer::new(1, audio.sample_rate, samples);
    sink.append(source);
    sink.play();

    println!("Playing audio... (press Ctrl+C to stop)");
    sink.sleep_until_end();

    Ok(())
}

fn test_asr() -> Result<(), Box<dyn std::error::Error>> {
    println!("Testing ASR (Speech Recognition)");
    println!("Note: This requires a microphone and will listen for 10 seconds");

    // Find default microphone
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| "No input device found")?;

    let device_name = device
        .description()
        .map(|d| d.name().to_string())
        .unwrap_or_default();
    println!("Using device: {}", device_name);

    let config = device.default_input_config()?;
    println!(
        "Input config: {} Hz, {} channels",
        config.sample_rate(),
        config.channels()
    );

    // Initialize recognizer
    let sense_config = SenseVoiceConfig {
        model: "./assets/module/sense_voice/model.int8.onnx".into(),
        tokens: "./assets/module/sense_voice/tokens.txt".into(),
        provider: Some("cpu".into()),
        ..Default::default()
    };

    let mut recognizer = SenseVoiceRecognizer::new(sense_config)?;
    println!("SenseVoice recognizer initialized");

    // Create audio stream
    let (tx, rx) = mpsc::sync_channel::<Vec<i16>>(10);

    let stream = device.build_input_stream(
        &config.clone().into(),
        move |data: &[f32], _: &_| {
            let mono: Vec<i16> = data.iter().map(|&s| (s * i16::MAX as f32) as i16).collect();
            let _ = tx.send(mono);
        },
        |e| eprintln!("Stream error: {}", e),
        None,
    )?;

    stream.play()?;
    println!("Listening... Speak now! (10 seconds)");

    // Listen for 10 seconds
    let start = std::time::Instant::now();
    let mut buffer = Vec::new();

    while start.elapsed().as_secs() < 10 {
        if let Ok(samples) = rx.recv_timeout(std::time::Duration::from_millis(100)) {
            buffer.extend(samples);

            // Process in chunks of 1600 samples (100ms at 16kHz)
            while buffer.len() >= 1600 {
                let chunk: Vec<i16> = buffer.drain(..1600).collect();
                let float_chunk: Vec<f32> =
                    chunk.iter().map(|&s| s as f32 / i16::MAX as f32).collect();

                let result = recognizer.transcribe(16000, &float_chunk);
                if !result.text.is_empty() {
                    println!("Recognized: {}", result.text);
                }
            }
        }
    }

    println!("Done listening!");
    Ok(())
}
