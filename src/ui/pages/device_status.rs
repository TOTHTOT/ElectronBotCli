use crate::app::App;
use crate::ui_components::create_block;
use ratatui::{prelude::*, widgets::*};

fn get_pc_battery() -> u32 {
    85
}

fn get_network_status() -> &'static str {
    "已连接"
}

fn status_color(ok: bool) -> Color {
    if ok {
        Color::Green
    } else {
        Color::Red
    }
}

pub fn render(frame: &mut Frame, area: Rect, app: &App, border_color: Color) {
    let is_connected = app.is_connected();
    let volume = app.voice_manager.volume();

    // 使用 Table 实现网格布局
    let table = Table::new(
        vec![
            Row::new(vec![
                Cell::from(Span::styled("连接状态", Style::new().fg(Color::Yellow))),
                Cell::from(Span::styled(
                    if is_connected {
                        "已连接"
                    } else {
                        "未连接"
                    },
                    Style::new().fg(status_color(is_connected)).bold(),
                )),
            ]),
            Row::new(vec![
                Cell::from(Span::styled("上位机电量", Style::new().fg(Color::Yellow))),
                Cell::from(Span::styled(
                    format!("{}%", get_pc_battery()),
                    Style::new().fg(status_color(get_pc_battery() > 50)),
                )),
            ]),
            Row::new(vec![
                Cell::from(Span::styled("网络状态", Style::new().fg(Color::Yellow))),
                Cell::from(Span::styled(
                    get_network_status(),
                    Style::new().fg(status_color(get_network_status() == "已连接")),
                )),
            ]),
            Row::new(vec![
                Cell::from(Span::styled("输入音量", Style::new().fg(Color::Yellow))),
                // 音量条
                Cell::from(Span::styled(
                    format!("{:─<20}", "│".repeat((volume / 5) as usize)),
                    Style::new().fg(Color::Cyan),
                )),
            ]),
            Row::new(vec![
                Cell::from(Span::styled(
                    "按 [Enter] 连接设备",
                    Style::new().fg(Color::Gray),
                )),
                Cell::from(Span::styled(
                    format!("{}", volume),
                    Style::new().fg(Color::Cyan),
                )),
            ]),
        ],
        &[Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)],
    )
    .column_spacing(2);
    let outer_block = create_block("操作说明".to_string(), border_color, border_color);
    let inner_area = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    let widget = Paragraph::new(Line::raw("")).alignment(Alignment::Left);
    frame.render_widget(widget, inner_area);

    let inner = area.inner(Margin {
        horizontal: 1,
        vertical: 1,
    });
    frame.render_widget(table, inner);
}
