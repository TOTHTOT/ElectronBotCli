use crate::app::App;
use crate::robot::{ServoState, SERVO_COUNT};
use crate::ui_components::{create_block, get_indicator};
use ratatui::{prelude::*, widgets::Paragraph};

pub fn render(frame: &mut Frame, area: Rect, app: &App, border_color: Color) {
    let outer_block = create_block("设备控制".to_string(), border_color, border_color);

    let inner_area = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    let chunks = Layout::new(
        Direction::Vertical,
        [Constraint::Length(3), Constraint::Min(0)],
    )
    .split(inner_area);

    render_info_bar(frame, chunks[0], border_color);
    render_joint_gauges(frame, chunks[1], app, border_color);
}

fn render_info_bar(frame: &mut Frame, area: Rect, border_color: Color) {
    let outer_block = create_block("操作说明".to_string(), border_color, border_color);
    let inner_area = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    let text = vec![Line::from_iter([Span::styled(
        "操作: [↑] 上一舵机  [↓] 下一舵机  [←] -1°  [→] +1°  [s] 截图保存  [Esc] 返回",
        Style::new().fg(Color::White),
    )])];

    let widget = Paragraph::new(text).style(Style::new().bg(Color::DarkGray));
    frame.render_widget(widget, inner_area);
}

fn render_joint_gauges(frame: &mut Frame, area: Rect, app: &App, border_color: Color) {
    let outer_block = create_block("关节控制".to_string(), border_color, border_color);

    let servo_height = (area.height as usize) / SERVO_COUNT;
    let extra_rows = (area.height as usize) % SERVO_COUNT;
    let inner_area = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    for i in 0..SERVO_COUNT {
        let row_height = if i < extra_rows {
            servo_height + 1
        } else {
            servo_height
        };

        let row_area = Rect::new(
            inner_area.x,
            inner_area.y + i as u16 * servo_height as u16 + (i as u16).min(extra_rows as u16),
            inner_area.width,
            row_height as u16,
        );

        render_single_joint(frame, row_area, app, i);
    }
}

fn render_single_joint(frame: &mut Frame, area: Rect, app: &App, index: usize) {
    let values = app.joint.values();
    let is_selected = index == app.joint.selected() && app.in_servo_mode;
    let value = values[index];
    let name = ServoState::name(index);
    let range_str = ServoState::range_str(index);

    let indicator = get_indicator(is_selected, is_selected); // 选中时作为编辑状态显示 ▶

    let color = if is_selected && app.in_servo_mode {
        Color::Cyan
    } else {
        Color::White
    };

    // 计算进度条
    let min = ServoState::min_angle(index);
    let max = ServoState::max_angle(index);
    let total_range = (max - min) as f32;
    let value_offset = (value - min) as f32;
    let percent = if total_range > 0.0 {
        ((value_offset / total_range) * 100.0) as u16
    } else {
        0
    };

    let bar_width = (area.width as usize).saturating_sub(35);
    let filled = percent * bar_width as u16 / 100;
    let empty = bar_width as u16 - filled;

    let bar = format!(
        "▏{}▎",
        "█".repeat(filled as usize) + &"░".repeat(empty as usize)
    );

    let text = vec![Line::from_iter([
        Span::styled(
            indicator.to_string(),
            Style::new().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {name}:"), Style::new().fg(color)),
        Span::styled(bar, Style::new().fg(color)),
        Span::styled(format!(" {value}°"), Style::new().fg(color)),
        Span::styled(format!(" [{range_str}]"), Style::new().fg(Color::DarkGray)),
    ])];

    let widget = Paragraph::new(text).style(Style::new().fg(Color::White));
    frame.render_widget(widget, area);
}
