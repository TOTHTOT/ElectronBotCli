use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

pub fn render(frame: &mut Frame, area: Rect) {
    let text = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  ElectronBot Controller",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("  版本: 0.1.0"),
        Line::from("  作者: Your Name"),
        Line::from(""),
        Line::from(Span::styled(
            "  ElectronBot 是一个桌面小机器人",
            Style::default().fg(Color::Gray),
        )),
        Line::from(Span::styled(
            "  本程序用于控制和配置设备",
            Style::default().fg(Color::Gray),
        )),
        Line::from(""),
        Line::from("  快捷键:"),
        Line::from("    ↑/k   向上"),
        Line::from("    ↓/j   向下"),
        Line::from("    Esc/q 退出"),
    ];

    let widget = Paragraph::new(text).block(
        Block::default()
            .title("关于")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green)),
    );

    frame.render_widget(widget, area);
}
