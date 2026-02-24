//! LLM 测试输入处理模块

use crate::app::App;
use crossterm::event::KeyCode;

/// LLM 测试事件
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmTestEvent {
    None,
}

/// 处理 LLM 测试模式的输入
pub fn handle(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Char(c) => {
            app.llm_test_state.input_text.push(c);
        }
        KeyCode::Backspace => {
            app.llm_test_state.input_text.pop();
        }
        KeyCode::Enter => {
            let input = app.llm_test_state.input_text.clone();
            if !input.is_empty() {
                app.is_processing
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                app.llm_test_state.output_text = "正在分析...".to_string();
                // 发送到 LLM 处理通道
                let _ = app.text_tx.send(input);
            }
        }
        KeyCode::Esc => {
            // 清除输入
            app.llm_test_state.input_text.clear();
            app.llm_test_state.output_text.clear();
            app.llm_test_state.current_mood = None;
        }
        _ => {}
    }
}
