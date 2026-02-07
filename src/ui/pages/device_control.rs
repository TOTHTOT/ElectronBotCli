use crate::app::{App, Expression, RobotEyes, ServoState, FRAME_WIDTH, SERVO_COUNT};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    // 外层 Block（包含所有内容）
    let outer_block = Block::new()
        .title("ElectronBot 设备控制")
        .title_style(Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::new().fg(Color::Blue));

    let inner_area = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    let chunks = Layout::new(
        Direction::Vertical,
        [Constraint::Length(3), Constraint::Min(0)],
    )
    .split(inner_area);

    // 操作说明（横向满）
    render_info_bar(frame, chunks[0]);

    // 舵机控制和LCD缓冲区（左右各50%）
    let main_chunks = Layout::new(
        Direction::Horizontal,
        [Constraint::Percentage(50), Constraint::Percentage(50)],
    )
    .split(chunks[1]);

    render_servo_gauges(frame, main_chunks[0], app);
    render_lcd_preview(frame, main_chunks[1], app);
}

fn render_info_bar(frame: &mut Frame, area: Rect) {
    let outer_block = Block::new()
        .title("操作说明")
        .title_style(Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::new().fg(Color::Blue));
    let inner_area = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    let text = vec![Line::from_iter([Span::styled(
        "操作: [↑/k] +1°  [↓/j] -1°  [a] -5°  [d] +5°  [h/l] 选择舵机  [Esc] 退出模式",
        Style::new().fg(Color::White),
    )])];

    let widget = Paragraph::new(text).style(Style::new().bg(Color::DarkGray));
    frame.render_widget(widget, inner_area);
}

fn render_servo_gauges(frame: &mut Frame, area: Rect, app: &App) {
    let outer_block = Block::new()
        .title("舵机控制")
        .title_style(Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::new().fg(Color::Blue));
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
    let percent = ServoState::percent(value);
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

    let text = vec![Line::from_iter([
        Span::styled(
            indicator.to_string(),
            Style::new().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {}:", name), Style::new().fg(color)),
        Span::styled(bar, Style::new().fg(color)),
        Span::styled(format!(" {:4}°", value), Style::new().fg(color)),
    ])];

    let widget = Paragraph::new(text).style(Style::new().fg(Color::White));
    frame.render_widget(widget, area);
}

fn render_lcd_preview(frame: &mut Frame, area: Rect, _app: &App) {
    let outer_block = Block::new()
        .title("RobotEyes")
        .title_style(Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::new().fg(Color::Blue));
    let inner_area = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    // 生成 RobotEyes 像素
    let mut eyes = RobotEyes::new();
    eyes.set_expression(Expression::Neutral);
    eyes.random_blink();
    let mut pixels = vec![0u8; FRAME_WIDTH * 240 * 3];
    eyes.generate_frame(&mut pixels);

    // 可用字符区域
    let avail_width = inner_area.width.saturating_sub(2) as usize;
    let avail_height = inner_area.height.saturating_sub(2) as usize;

    // 终端字符宽高比约 2:1
    // 240x240 像素，根据可用高度缩放
    let image_size = 240;
    let char_aspect = 2.0;

    // 计算缩放比例（像素 -> 字符）
    let scale = image_size as f32 / avail_height as f32;
    let scaled_width = (image_size as f32 / scale / char_aspect) as usize;

    // 居中偏移
    let offset_x = (avail_width.saturating_sub(scaled_width)) / 2;

    let chars = [' ', '░', '▒', '█'];

    let mut lines = Vec::new();
    for py in 0..avail_height {
        let pixel_y = (py as f32 * scale) as usize;
        let mut line = String::new();
        for px in 0..avail_width {
            if pixel_y < image_size {
                // 计算对应像素位置
                let scaled_px = if px >= offset_x { px - offset_x } else { px };
                let pixel_x = (scaled_px as f32 * char_aspect * scale) as usize;

                if pixel_x < image_size {
                    let idx = (pixel_y * 240 + pixel_x) * 3;
                    if idx + 2 < pixels.len() {
                        let r = pixels[idx];
                        let g = pixels[idx + 1];
                        let b = pixels[idx + 2];
                        let brightness = (r as u16 + g as u16 + b as u16) / 3;
                        let char_idx = ((brightness / 64) as usize).min(3);
                        line.push(chars[char_idx]);
                    } else {
                        line.push(' ');
                    }
                } else {
                    line.push(' ');
                }
            } else {
                line.push(' ');
            }
        }
        lines.push(Line::raw(line));
    }

    let widget = Paragraph::new(lines).style(Style::new().fg(Color::Green));
    frame.render_widget(widget, inner_area);
}
