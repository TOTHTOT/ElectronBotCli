//! TTS 测试页面
//!
//! 用于测试 TTS 语音合成功能

use crate::ui_components::create_block;
use ratatui::widgets::{Block, Paragraph, Wrap};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    prelude::Stylize,
    style::Color,
    style::Style,
    Frame,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Default)]
pub struct TtsTestState {
    pub input_text: String,
    pub output_text: String,
    pub speed: f32,                  // 0.5 ~ 2.0, 默认 1.0
    pub is_streaming: bool,          // true=流式, false=阻塞
    pub is_playing: Arc<AtomicBool>, // 使用 AtomicBool 以便线程间共享
}

/// TTS 测试页面
pub fn render(frame: &mut Frame, area: Rect, state: &TtsTestState, border_color: Color) {
    let outer_block = create_block("TTS 测试".to_string(), border_color, border_color);
    let inner_area = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5), // 输入区域
            Constraint::Length(5), // 控制面板
            Constraint::Min(3),    // 输出区域
            Constraint::Length(3), // 状态显示
        ])
        .split(inner_area);

    // 输入框
    let input_style = Style::default().fg(Color::Yellow);
    let input_box = Paragraph::new(state.input_text.as_str()).block(
        Block::bordered()
            .title("输入 (按回车播放)")
            .style(input_style),
    );
    frame.render_widget(input_box, chunks[0]);

    // 控制面板：速度 + 模式
    let speed_text = format!("速度: {:.1}", state.speed);
    let mode_text = if state.is_streaming {
        "流式"
    } else {
        "阻塞"
    };
    let control_text = format!("{speed_text}  |  模式: [{mode_text}]  (按 M 切换)");
    let control_box = Paragraph::new(control_text.as_str())
        .block(Block::bordered().title("控制"))
        .alignment(Alignment::Left);
    frame.render_widget(control_box, chunks[1]);

    // 输出区域
    let output_text = if state.output_text.is_empty() {
        "等待输入...".to_string()
    } else {
        state.output_text.clone()
    };
    let output_box = Paragraph::new(output_text)
        .block(Block::bordered().title("输出"))
        .wrap(Wrap { trim: true });
    frame.render_widget(output_box, chunks[2]);

    // 状态显示
    let is_playing = state.is_playing.load(Ordering::SeqCst);
    let status_text = if is_playing {
        "状态: 播放中..."
    } else if state.output_text.is_empty() {
        "状态: 就绪"
    } else {
        "状态: 完成"
    };

    let status = Paragraph::new(status_text)
        .bold()
        .alignment(Alignment::Center);
    frame.render_widget(status, chunks[3]);
}
