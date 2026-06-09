//! TTS 测试 ViewModel

use crate::app::App;
use std::sync::atomic::Ordering;

pub struct TtsTestViewModel {
    pub input_text: String,
    pub output_text: String,
    pub speed: f32,
    pub is_streaming: bool,
    pub is_playing: bool,
}

impl TtsTestViewModel {
    pub fn from_app(app: &App) -> Self {
        // 从 AtomicBool 读取播放状态
        let is_playing = app.ai.tts_test_state.is_playing.load(Ordering::SeqCst);
        Self {
            input_text: app.ai.tts_test_state.input_text.clone(),
            output_text: app.ai.tts_test_state.output_text.clone(),
            speed: app.ai.tts_test_state.speed,
            is_streaming: app.ai.tts_test_state.is_streaming,
            is_playing,
        }
    }
}
