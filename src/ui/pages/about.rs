use crate::ui_components::create_block;
use ratatui::{prelude::*, widgets::Paragraph};

fn get_app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn render(frame: &mut Frame, area: Rect) {
    let version = get_app_version();

    let text = vec![
        Line::raw(""),
        Line::from_iter([Span::styled(
            "  ElectronBot Controller Command Line Tool",
            Style::new().add_modifier(Modifier::BOLD),
        )]),
        Line::raw(""),
        Line::from_iter([Span::styled(
            format!("  版本: {version}"),
            Style::new().fg(Color::White),
        )]),
        Line::raw("  作者: TOTHTOT"),
        Line::raw(""),
        Line::from_iter([Span::styled(
            "  ElectronBot 是一个桌面小机器人",
            Style::new().fg(Color::Gray),
        )]),
        Line::from_iter([Span::styled(
            "  本程序用于控制和配置设备",
            Style::new().fg(Color::Gray),
        )]),
        Line::raw(""),
        Line::raw("  快捷键:"),
        Line::raw("    ↑/k   向上"),
        Line::raw("    ↓/j   向下"),
        Line::raw("    Esc/q 退出"),
    ];
    let outer_block = create_block("关于".to_string(), Color::Green, Color::Green);
    let inner_area = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    let widget = Paragraph::new(text);
    frame.render_widget(widget, inner_area);
}
