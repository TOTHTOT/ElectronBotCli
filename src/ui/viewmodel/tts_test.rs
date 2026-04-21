//! TTS 测试 ViewModel

use crate::app::App;

pub struct TtsTestViewModel {
    pub input_text: String,
    pub output_text: String,
    pub speed: f32,
    pub is_streaming: bool,
    pub is_playing: bool,
}

impl TtsTestViewModel {
    pub fn from_app(app: &App) -> Self {
        Self {
            input_text: app.ai.tts_test_state.input_text.clone(),
            output_text: app.ai.tts_test_state.output_text.clone(),
            speed: app.ai.tts_test_state.speed,
            is_streaming: app.ai.tts_test_state.is_streaming,
            is_playing: app.ai.tts_test_state.is_playing,
        }
    }
}
