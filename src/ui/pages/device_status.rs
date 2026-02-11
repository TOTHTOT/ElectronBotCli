use crate::app::App;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

/// 获取上位机电量
fn get_pc_battery() -> u32 {
    // TODO: 实际项目中可以从系统 API 获取
    85 // 模拟返回 85%
}

/// 获取网络状态
fn get_network_status() -> &'static str {
    // TODO: 实际项目中可以从系统 API 获取
    "已连接"
}

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

    let pc_battery = get_pc_battery();
    let battery_color = if pc_battery > 50 {
        Color::Green
    } else if pc_battery > 20 {
        Color::Yellow
    } else {
        Color::Red
    };

    let network_status = get_network_status();
    let network_color = if network_status == "已连接" {
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
        Line::from_iter([Span::styled(
            "  按 [Enter] 连接设备",
            Style::new().fg(Color::Gray),
        )]),
        Line::raw(""),
        Line::from_iter([Span::styled("上位机电量:", Style::new().fg(Color::Yellow))]),
        Line::from_iter([Span::styled(
            format!("  {}%", pc_battery),
            Style::new().fg(battery_color),
        )]),
        Line::raw(""),
        Line::from_iter([Span::styled("网络状态:", Style::new().fg(Color::Yellow))]),
        Line::from_iter([Span::styled(
            format!("  {}", network_status),
            Style::new().fg(network_color),
        )]),
    ];

    let widget = Paragraph::new(text).block(
        Block::new()
            .title("设备状态")
            .borders(Borders::ALL)
            .border_style(Style::new().fg(Color::Green)),
    );

    frame.render_widget(widget, area);
}
