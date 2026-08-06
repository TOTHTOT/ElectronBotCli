//! LLM 测试页面
//!
//! 用于手动发送文本并调用 LLM 模型生成回复

use crate::ui_components::create_block;
use ele_bot_proto::Mood;
use ratatui::widgets::{Block, Paragraph, Wrap};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    prelude::Stylize,
    style::Color,
    style::Style,
    Frame,
};

#[derive(Default)]
pub struct LlmTestState {
    pub input_text: String,
    pub output_text: String,
    pub current_mood: Option<Mood>,
}

/// LLM 测试页面
pub fn render(
    frame: &mut Frame,
    area: Rect,
    state: &LlmTestState,
    is_processing: bool,
    border_color: Color,
) {
    let outer_block = create_block("LLM 情感测试".to_string(), border_color, border_color);
    let inner_area = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5), // 输入区域
            Constraint::Min(3),    // 输出区域
            Constraint::Length(3), // 状态/情感显示
        ])
        .split(inner_area);
    // 输入框
    let input_style = Style::default().fg(Color::Yellow);
    let input_box = Paragraph::new(state.input_text.as_str()).block(
        Block::bordered()
            .title("输入 (回车发送, F2 清空记忆)")
            .style(input_style),
    );
    frame.render_widget(input_box, chunks[0]);

    // 输出区域
    let output_text = if state.output_text.is_empty() {
        "等待输入...".to_string()
    } else {
        state.output_text.clone()
    };
    let output_box = Paragraph::new(output_text)
        .block(Block::bordered().title("输出"))
        .wrap(Wrap { trim: true });
    frame.render_widget(output_box, chunks[1]);

    // 状态/情感显示
    let status_text = if is_processing {
        "状态: 处理中..."
    } else if let Some(mood) = state.current_mood {
        match mood {
            Mood::Happy => "情感: 开心 😊",
            Mood::Sad => "情感: 难过 😢",
            Mood::Angry => "情感: 生气 😠",
            Mood::Surprise => "情感: 惊讶 😲",
            Mood::Confuse => "情感: 害怕 😨",
            Mood::Default => "情感: 中性 😐",
            Mood::Loading => "情感: 加载中 ⏳",
        }
    } else {
        "状态: 就绪"
    };

    let status = Paragraph::new(status_text)
        .bold()
        .alignment(Alignment::Center);
    frame.render_widget(status, chunks[2]);
}
