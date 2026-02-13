use crate::app::config::AppConfig;
use crate::ui_components::{create_block, get_indicator};
use ratatui::{prelude::*, widgets::Paragraph};

pub fn render(
    frame: &mut Frame,
    area: Rect,
    selected: usize,
    config: &AppConfig,
    in_edit: bool,
    edit_buffer: &str,
) {
    let outer_block = create_block("设置".to_string(), Color::Green, Color::Green);
    let inner_area = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    let chunks = Layout::new(
        Direction::Vertical,
        [Constraint::Length(3), Constraint::Min(0)],
    )
    .split(inner_area);

    render_info_bar(frame, chunks[0], in_edit);
    render_settings_list(frame, chunks[1], selected, config, in_edit, edit_buffer);
}

fn render_info_bar(frame: &mut Frame, area: Rect, in_edit: bool) {
    let outer_block = create_block("操作说明".to_string(), Color::Green, Color::Cyan);
    let inner_area = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    let text = if in_edit {
        "操作: [Enter] 保存  [Esc] 取消  [Backspace] 删除字符"
    } else {
        "操作: [↑/↓] 选择  [Enter] 编辑  [Esc] 退出"
    };

    let line = vec![Line::from_iter([Span::styled(
        text,
        Style::new().fg(Color::White),
    )])];

    let widget = Paragraph::new(line).style(Style::new().bg(Color::DarkGray));
    frame.render_widget(widget, inner_area);
}

fn render_settings_list(
    frame: &mut Frame,
    area: Rect,
    selected: usize,
    config: &AppConfig,
    in_edit: bool,
    edit_buffer: &str,
) {
    let outer_block = create_block("配置项".to_string(), Color::Green, Color::Cyan);

    let inner_area = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    let items = [
        ("Wifi名称", config.wifi_ssid.as_str()),
        ("Wifi密码", config.wifi_password.as_str()),
        ("麦克风名称", config.speech_name.as_str()),
    ];

    // 渲染每个设置项
    for (i, (label, value)) in items.iter().enumerate() {
        let y = inner_area.y + i as u16;
        let item_area = Rect::new(inner_area.x, y, inner_area.width, 1);

        render_setting_item(
            frame,
            item_area,
            label,
            value,
            i == selected,
            in_edit && i == selected,
            edit_buffer,
        );
    }
}

/// 渲染设置项
fn render_setting_item(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    value: &str,
    is_selected: bool,
    is_editing: bool,
    edit_buffer: &str,
) {
    let indicator = get_indicator(is_selected, is_editing);

    let color = if is_selected {
        Color::Cyan
    } else {
        Color::White
    };

    let display_value = if is_editing { edit_buffer } else { value };

    let text = vec![Line::from_iter([
        Span::styled(
            indicator.to_string(),
            Style::new().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {label}: "), Style::new().fg(color)),
        Span::styled(
            display_value,
            if is_editing {
                Style::new().fg(Color::Black).bg(Color::White)
            } else if value.is_empty() {
                Style::new().fg(Color::DarkGray)
            } else {
                Style::new().fg(Color::Yellow)
            },
        ),
    ])];

    let widget = Paragraph::new(text).style(Style::new().fg(Color::White));
    frame.render_widget(widget, area);
}
