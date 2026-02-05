use crate::app::App;
use crate::device::DeviceState;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let (status_text, status_color) = match app.device.state() {
        DeviceState::Disconnected => ("未连接", Color::Red),
        DeviceState::Connected(port) => (port.as_str(), Color::Green),
        DeviceState::Error(msg) => (msg.as_str(), Color::Red),
    };

    let is_connected = matches!(app.device.state(), DeviceState::Connected(_));

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
        Line::raw(format!("  帧大小: {} bytes", crate::device::FRAME_SIZE)),
        Line::raw(format!("  缓冲区: {}", crate::device::BUFFER_COUNT)),
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
        Line::from_iter([Span::styled("可用串口:", Style::new().fg(Color::Yellow))]),
        Line::raw("  (请在设备控制页面连接)"),
    ];

    let widget = Paragraph::new(text).block(
        Block::new()
            .title("设备状态")
            .borders(Borders::ALL)
            .border_style(Style::new().fg(Color::Green)),
    );

    frame.render_widget(widget, area);
}
