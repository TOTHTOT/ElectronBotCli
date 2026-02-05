use crate::app::{App, ServoState, SERVO_COUNT};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};
use std::default::Default;
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    // 外层 Block（包含所有内容）
    let outer_block = Block::default()
        .title("ElectronBot 设备控制")
        .title_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));

    let inner_area = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // 操作说明
            Constraint::Min(0),    // 舵机控制 + LCD缓冲区
        ])
        .split(inner_area);

    // 操作说明（横向满）
    render_info_bar(frame, chunks[0]);

    // 舵机控制和LCD缓冲区（左右各50%）
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50), // 舵机控制（左侧）
            Constraint::Percentage(50), // LCD缓冲区（右侧）
        ])
        .split(chunks[1]);

    render_servo_gauges(frame, main_chunks[0], app);
    render_lcd_preview(frame, main_chunks[1], app);
}

fn render_info_bar(frame: &mut Frame, area: Rect) {
    let outer_block = Block::default()
        .title("操作说明")
        .title_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));
    let inner_area = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    let text = vec![Line::from(Span::styled(
        "操作: [↑/k] +1°  [↓/j] -1°  [a] -5°  [d] +5°  [h/l] 选择舵机  [Esc] 退出模式",
        Style::default().fg(Color::White),
    ))];

    let widget = Paragraph::new(text).style(Style::default().bg(Color::DarkGray));
    frame.render_widget(widget, inner_area);
}

fn render_servo_gauges(frame: &mut Frame, area: Rect, app: &App) {
    let outer_block = Block::default()
        .title("舵机控制")
        .title_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));
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

        render_single_servo(frame, row_area, app, i);
    }
}

fn render_single_servo(frame: &mut Frame, area: Rect, app: &App, index: usize) {
    let servo = &app.servo_state;
    let is_selected = index == servo.selected && app.in_servo_mode;
    let value = servo.values[index];
    let percent = ServoState::to_percent(value);
    let name = ServoState::servo_name(index);

    let indicator = if is_selected {
        if app.in_servo_mode {
            "▶"
        } else {
            "○"
        }
    } else {
        " "
    };

    let color = if is_selected && app.in_servo_mode {
        Color::Cyan
    } else {
        Color::White
    };

    let bar_width = (area.width as usize).saturating_sub(20);
    let filled = percent * bar_width as u16 / 100;
    let empty = bar_width as u16 - filled;

    let bar = format!(
        "▏{}▎",
        "█".repeat(filled as usize) + &"░".repeat(empty as usize)
    );

    let text = vec![Line::from(vec![
        Span::styled(
            format!("{}", indicator),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {}:", name), Style::default().fg(color)),
        Span::styled(bar, Style::default().fg(color)),
        Span::styled(format!(" {:4}°", value), Style::default().fg(color)),
    ])];

    let widget = Paragraph::new(text).style(Style::default().fg(Color::White));
    frame.render_widget(widget, area);
}

fn render_lcd_preview(frame: &mut Frame, area: Rect, _app: &App) {
    let outer_block = Block::default()
        .title("Lcd 缓冲区")
        .title_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));
    let inner_area = outer_block.inner(area);
    frame.render_widget(outer_block, area);
    let width = (inner_area.width as usize).saturating_sub(2);
    let height = (inner_area.height as usize).saturating_sub(2);
    let chars = [' ', '░', '▒', '█'];

    let mut lines = Vec::new();
    for y in 0..height {
        let mut line = String::new();
        for x in 0..width {
            let v = ((x + y) as f32 / (width + height) as f32) * 255.0;
            let idx = ((v / 64.0) as usize).min(3);
            line.push(chars[idx]);
        }
        lines.push(Line::from(line));
    }

    let widget = Paragraph::new(lines).style(Style::default().fg(Color::Green));
    frame.render_widget(widget, inner_area);
}
