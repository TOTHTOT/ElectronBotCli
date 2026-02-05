use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

pub fn render(frame: &mut Frame, area: Rect) {
    let text = vec![
        Line::raw(""),
        Line::raw("  [1] 串口设置"),
        Line::raw("  [2] 显示设置"),
        Line::raw("  [3] 动作设置"),
        Line::raw(""),
        Line::from_iter([Span::styled(
            "  按数字键选择设置项",
            Style::new().fg(Color::Gray),
        )]),
    ];

    let widget = Paragraph::new(text).block(
        Block::new()
            .title("设置")
            .borders(Borders::ALL)
            .border_style(Style::new().fg(Color::Green)),
    );

    frame.render_widget(widget, area);
}
