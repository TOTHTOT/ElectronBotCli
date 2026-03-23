use crate::app::App;
use boteyes::Mood;

pub struct LlmTestViewModel {
    pub input_text: String,
    pub output_text: String,
    pub current_mood: Option<Mood>,
    pub is_processing: bool,
}

impl LlmTestViewModel {
    pub fn from_app(app: &App) -> Self {
        Self {
            input_text: app.ai.llm_test_state.input_text.clone(),
            output_text: app.ai.llm_test_state.output_text.clone(),
            current_mood: app.ai.llm_test_state.current_mood,
            is_processing: app
                .ai
                .is_processing
                .load(std::sync::atomic::Ordering::Relaxed),
        }
    }
}
