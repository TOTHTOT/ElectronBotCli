use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};
use crate::app::App;
use crate::device::DeviceState;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let (status_text, status_color) = match app.device.state() {
        DeviceState::Disconnected => ("未连接", Color::Red),
        DeviceState::Connected(port) => (port.as_str(), Color::Green),
        DeviceState::Error(msg) => (msg.as_str(), Color::Red),
    };

    let is_connected = matches!(app.device.state(), DeviceState::Connected(_));

    let text = vec![
        Line::from(""),
        Line::from(Span::styled(
            "连接状态: ",
            Style::default().fg(Color::Yellow),
        )),
        Line::from(Span::styled(
            format!("  {}", status_text),
            Style::default().fg(status_color),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "设备信息:",
            Style::default().fg(Color::Yellow),
        )),
        if is_connected {
            Line::from("  型号: ElectronBot")
        } else {
            Line::from("  型号: -")
        },
        if is_connected {
            Line::from("  固件版本: v1.0.0")
        } else {
            Line::from("  固件版本: -")
        },
        Line::from(""),
        Line::from(Span::styled(
            "帧数据:",
            Style::default().fg(Color::Yellow),
        )),
        Line::from(format!("  帧大小: {} bytes", crate::device::FRAME_SIZE)),
        Line::from(format!("  缓冲区: {}", crate::device::BUFFER_COUNT)),
        Line::from(""),
        if is_connected {
            Line::from(Span::styled(
                "  已连接设备，数据传输中...",
                Style::default().fg(Color::Green),
            ))
        } else {
            Line::from(Span::styled(
                "  按 [Enter] 连接设备",
                Style::default().fg(Color::Gray),
            ))
        },
        Line::from(""),
        Line::from(Span::styled(
            "可用串口:",
            Style::default().fg(Color::Yellow),
        )),
        Line::from("  (请在设备控制页面连接)"),
    ];

    let widget = Paragraph::new(text).block(
        Block::default()
            .title("设备状态")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green)),
    );

    frame.render_widget(widget, area);
}
