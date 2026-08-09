//! LLM 测试输入处理模块

use crate::app::App;
use crossterm::event::{KeyCode, KeyModifiers};

/// 处理 LLM 测试模式的输入
pub fn handle(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    // Ctrl+U 清空输入框 (先于 Char 通配分支拦截)
    if modifiers.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('u') {
        app.ai.llm_test_state.input.clear();
        return;
    }
    // 部分终端把 Backspace 发成 ^H (0x08), crossterm 解为 Char('h')+CONTROL,
    // 不拦截会被 Char 通配分支当成字符 'h' 插入
    if modifiers.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('h') {
        app.ai.llm_test_state.input.delete_back();
        return;
    }
    match code {
        KeyCode::Char(c) => app.ai.llm_test_state.input.insert_char(c),
        KeyCode::Left => app.ai.llm_test_state.input.move_left(1),
        KeyCode::Right => app.ai.llm_test_state.input.move_right(1),
        KeyCode::Home => app.ai.llm_test_state.input.move_to_start(),
        KeyCode::End => app.ai.llm_test_state.input.move_to_end(),
        KeyCode::Backspace => {
            app.ai.llm_test_state.input.delete_back();
        }
        KeyCode::Delete => {
            app.ai.llm_test_state.input.delete_forward();
        }
        KeyCode::Enter => {
            let input = app.ai.llm_test_state.input.text().to_string();
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
