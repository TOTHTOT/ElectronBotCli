use crate::ui::viewmodel::DeviceStatusViewModel;
use crate::ui_components::create_block;
use ratatui::{prelude::*, widgets::*};

fn status_color(ok: bool) -> Color {
    if ok {
        Color::Green
    } else {
        Color::Red
    }
}

pub fn render(frame: &mut Frame, area: Rect, vm: &DeviceStatusViewModel, border_color: Color) {
    // 使用 Table 实现网格布局
    let table = Table::new(
        vec![
            Row::new(vec![
                Cell::from(Span::styled("连接状态", Style::new().fg(Color::Yellow))),
                Cell::from(Span::styled(
                    if vm.is_connected {
                        "已连接"
                    } else {
                        "未连接"
                    },
                    Style::new().fg(status_color(vm.is_connected)).bold(),
                )),
            ]),
            Row::new(vec![
                Cell::from(Span::styled("上位机电量", Style::new().fg(Color::Yellow))),
                Cell::from(Span::styled(
                    format!("{}%", vm.battery),
                    Style::new().fg(status_color(vm.battery > 50)),
                )),
            ]),
            Row::new(vec![
                Cell::from(Span::styled("网络状态", Style::new().fg(Color::Yellow))),
                Cell::from(Span::styled(
                    vm.network,
                    Style::new().fg(status_color(vm.network == "已连接")),
                )),
            ]),
            Row::new(vec![
                Cell::from(Span::styled("输入音量", Style::new().fg(Color::Yellow))),
                // 音量条
                Cell::from(Span::styled(
                    format!(
                        "{: <20}",
                        "█".repeat((vm.volume / 5).min(20) as usize)
                    ),
                    Style::new().fg(Color::Cyan),
                )),
            ]),
            Row::new(vec![
                Cell::from(Span::styled(
                    "按 [Enter] 连接设备",
                    Style::new().fg(Color::Gray),
                )),
                Cell::from(Span::styled(
                    format!("{}", vm.volume),
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
