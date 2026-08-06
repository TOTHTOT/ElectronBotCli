//! LLM 测试输入处理模块

use crate::app::App;
use crossterm::event::KeyCode;

/// 处理 LLM 测试模式的输入
pub fn handle(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Char(c) => {
            app.ai.llm_test_state.input_text.push(c);
        }
        KeyCode::Backspace => {
            app.ai.llm_test_state.input_text.pop();
        }
        KeyCode::Enter => {
            let input = app.ai.llm_test_state.input_text.clone();
            if !input.is_empty() {
                app.ai
                    .is_processing
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                app.ai.llm_test_state.output_text = "正在分析...".to_string();
                // 发送到服务端
                app.send_llm_text(input);
            }
        }
        KeyCode::Esc => {
            // Esc 由 Route 层处理退到 Nav, 此处不再清空缓冲
            // (AiState 保持 long-lived, 重新进入可看到上次输入)
        }
        KeyCode::F(2) => {
            // 清空对话历史与个人记忆 (整体清空入口, 弹确认框)
            app.confirm_clear_llm_memory();
        }
        _ => {}
    }
}
