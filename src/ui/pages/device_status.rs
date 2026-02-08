use crate::app::App;
use crate::robot::{BUFFER_COUNT, FRAME_SIZE};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let is_connected = app.is_connected();

    let status_text = if is_connected {
        "已连接"
    } else {
        "未连接"
    };
    let status_color = if is_connected {
        Color::Green
    } else {
        Color::Red
    };

    let text = vec![
        Line::raw(""),
        Line::from_iter([Span::styled("连接状态: ", Style::new().fg(Color::Yellow))]),
        Line::from_iter([Span::styled(
            format!("  {}", status_text),
            Style::new().fg(status_color),
        )]),
        Line::raw(""),
        Line::from_iter([Span::styled("设备信息:", Style::new().fg(Color::Yellow))]),
        if is_connected {
            Line::raw("  型号: ElectronBot")
        } else {
            Line::raw("  型号: -")
        },
        if is_connected {
            Line::raw("  固件版本: v1.0.0")
        } else {
            Line::raw("  固件版本: -")
        },
        Line::raw(""),
        Line::from_iter([Span::styled("帧数据:", Style::new().fg(Color::Yellow))]),
        Line::raw(format!("  帧大小: {} bytes", FRAME_SIZE)),
        Line::raw(format!("  缓冲区: {}", BUFFER_COUNT)),
        Line::raw(""),
        if is_connected {
            Line::from_iter([Span::styled(
                "  已连接设备，数据传输中...",
                Style::new().fg(Color::Green),
            )])
        } else {
            Line::from_iter([Span::styled(
                "  按 [Enter] 连接设备",
                Style::new().fg(Color::Gray),
            )])
        },
        Line::raw(""),
        Line::from_iter([Span::styled("通信方式:", Style::new().fg(Color::Yellow))]),
        Line::raw("  USB (VID: 0x1001, PID: 0x8023)"),
    ];

    let widget = Paragraph::new(text).block(
        Block::new()
            .title("设备状态")
            .borders(Borders::ALL)
            .border_style(Style::new().fg(Color::Green)),
    );

    frame.render_widget(widget, area);
}
