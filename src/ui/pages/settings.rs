use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

pub fn render(frame: &mut Frame, area: Rect) {
    let text = vec![
        Line::from(""),
        Line::from("  [1] 串口设置"),
        Line::from("  [2] 显示设置"),
        Line::from("  [3] 动作设置"),
        Line::from(""),
        Line::from(Span::styled(
            "  按数字键选择设置项",
            Style::default().fg(Color::Gray),
        )),
    ];

    let widget = Paragraph::new(text).block(
        Block::default()
            .title("设置")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green)),
    );

    frame.render_widget(widget, area);
}
