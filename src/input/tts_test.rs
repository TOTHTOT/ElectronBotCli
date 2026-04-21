//! TTS 测试输入处理模块

use crate::app::App;
use crossterm::event::KeyCode;
use std::sync::atomic::Ordering;

/// TTS 测试事件
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TtsTestEvent {
    None,
}

/// 处理 TTS 测试模式的输入
pub fn handle(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Char('m') | KeyCode::Char('M') => {
            app.ai.tts_test_state.is_streaming = !app.ai.tts_test_state.is_streaming;
            let mode = if app.ai.tts_test_state.is_streaming {
                "流式"
            } else {
                "阻塞"
            };
            app.ai.tts_test_state.output_text = format!("切换到 {} 模式", mode);
        }
        KeyCode::Up | KeyCode::Char('+') | KeyCode::Char('=') => {
            let new_speed = (app.ai.tts_test_state.speed + 0.1).min(2.0);
            app.ai.tts_test_state.speed = new_speed;
            app.ai.tts_test_state.output_text = format!("速度: {:.1}", new_speed);
        }
        KeyCode::Down | KeyCode::Char('-') | KeyCode::Char('_') => {
            let new_speed = (app.ai.tts_test_state.speed - 0.1).max(0.5);
            app.ai.tts_test_state.speed = new_speed;
            app.ai.tts_test_state.output_text = format!("速度: {:.1}", new_speed);
        }
        KeyCode::Backspace => {
            app.ai.tts_test_state.input_text.pop();
        }
        KeyCode::Enter => {
            let input = app.ai.tts_test_state.input_text.clone();
            if !input.is_empty() && !app.ai.tts_test_state.is_playing.load(Ordering::SeqCst) {
                // 设置正在播放
                app.ai
                    .tts_test_state
                    .is_playing
                    .store(true, Ordering::SeqCst);
                app.ai.tts_test_state.output_text = "正在播放...".to_string();

                // 在新线程中执行 TTS 播放
                let is_streaming = app.ai.tts_test_state.is_streaming;
                let speed = app.ai.tts_test_state.speed;
                let voice_manager = app.ai.voice_manager.clone();
                let is_playing_flag = app.ai.tts_test_state.is_playing.clone();

                std::thread::spawn(move || {
                    let result = if is_streaming {
                        voice_manager.as_ref().map_or_else(
                            || Err(anyhow::anyhow!("Voice manager not available")),
                            |vm| vm.speak_streaming(&input, speed, None),
                        )
                    } else {
                        voice_manager.as_ref().map_or_else(
                            || Err(anyhow::anyhow!("Voice manager not available")),
                            |vm| vm.speak(&input, speed, None),
                        )
                    };

                    if let Err(e) = result {
                        log::warn!("TTS playback failed: {e:?}");
                    }

                    is_playing_flag.store(false, Ordering::SeqCst);
                });
            }
        }
        KeyCode::Esc => {
            // 清除输入
            app.ai.tts_test_state.input_text.clear();
            app.ai.tts_test_state.output_text.clear();
            app.ai
                .tts_test_state
                .is_playing
                .store(false, Ordering::SeqCst);
        }
        KeyCode::Char(c) => {
            app.ai.tts_test_state.input_text.push(c);
        }
        _ => {}
    }
}
