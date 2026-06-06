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
    border_color: Color,
    in_device_selection: bool,
    device_selection_index: usize,
    input_devices: &[String],
    output_devices: &[String],
) {
    let outer_block = create_block("设置".to_string(), border_color, border_color);
    let inner_area = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    let chunks = Layout::new(
        Direction::Vertical,
        [Constraint::Length(3), Constraint::Min(0)],
    )
    .split(inner_area);

    render_info_bar(
        frame,
        chunks[0],
        in_edit,
        in_device_selection,
        border_color,
    );
    render_settings_list(
        frame,
        chunks[1],
        selected,
        config,
        in_edit,
        edit_buffer,
        border_color,
        in_device_selection,
        device_selection_index,
        input_devices,
        output_devices,
    );
}

fn render_info_bar(
    frame: &mut Frame,
    area: Rect,
    in_edit: bool,
    in_device_selection: bool,
    border_color: Color,
) {
    let outer_block = create_block("操作说明".to_string(), border_color, border_color);
    let inner_area = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    let text = if in_device_selection {
        "操作: [↑/↓] 选择设备  [Enter] 确认  [Esc] 取消"
    } else if in_edit {
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
    border_color: Color,
    in_device_selection: bool,
    device_selection_index: usize,
    input_devices: &[String],
    output_devices: &[String],
) {
    let outer_block = create_block("配置项".to_string(), border_color, Color::Cyan);

    let inner_area = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    let output_display = if config.output_device.is_empty() {
        "(默认)"
    } else {
        config.output_device.as_str()
    };

    let items: [(&str, &str, bool); 4] = [
        ("Wifi名称", config.wifi_ssid.as_str(), false),
        ("Wifi密码", config.wifi_password.as_str(), false),
        ("输入设备", config.speech_name.as_str(), true),
        ("输出设备", output_display, true),
    ];

    // 渲染每个设置项
    for (i, (label, value, is_device)) in items.iter().enumerate() {
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
            *is_device,
        );
    }

    // 设备选择模式：在设置项下方内联渲染设备列表
    if in_device_selection {
        let devices = if selected == 2 {
            input_devices
        } else if selected == 3 {
            output_devices
        } else {
            &[]
        };

        // 设备列表从第 5 行开始（4 个设置项之后）
        let list_start_y = inner_area.y + items.len() as u16;
        let list_height = (devices.len() as u16 + 3).max(3); // 标题 + 项 + 边框
        let remaining_height = inner_area.height.saturating_sub(items.len() as u16);
        let render_height = list_height.min(remaining_height);
        if render_height == 0 {
            return;
        }
        let list_area = Rect::new(
            inner_area.x,
            list_start_y,
            inner_area.width,
            render_height,
        );
        render_device_list(frame, list_area, devices, device_selection_index, border_color);
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
    is_device: bool,
) {
    let indicator = get_indicator(is_selected, is_editing);

    let color = if is_selected {
        Color::Cyan
    } else {
        Color::White
    };

    let display_value = if is_editing { edit_buffer } else { value };

    // 设备项在值后追加 ▼ 提示
    let value_suffix = if is_device && !is_editing { " ▼" } else { "" };

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
        Span::styled(value_suffix, Style::new().fg(Color::DarkGray)),
    ])];

    let widget = Paragraph::new(text).style(Style::new().fg(Color::White));
    frame.render_widget(widget, area);
}

/// 渲染可用设备列表（内联展开）
fn render_device_list(
    frame: &mut Frame,
    area: Rect,
    devices: &[String],
    selected_index: usize,
    border_color: Color,
) {
    let block = create_block("可用设备".to_string(), border_color, Color::Yellow);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if devices.is_empty() {
        let line = vec![Line::from_iter([Span::styled(
            "  (无可用设备)",
            Style::new().fg(Color::DarkGray),
        )])];
        frame.render_widget(Paragraph::new(line), inner);
        return;
    }

    for (i, name) in devices.iter().enumerate() {
        if i as u16 >= inner.height {
            break;
        }
        let item_area = Rect::new(inner.x, inner.y + i as u16, inner.width, 1);
        let is_selected = i == selected_index;
        let prefix = if is_selected { "▶ " } else { "  " };
        let style = if is_selected {
            Style::new().fg(Color::Green).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(Color::White)
        };
        let line = vec![Line::from_iter([Span::styled(
            format!("{prefix}{name}"),
            style,
        )])];
        frame.render_widget(Paragraph::new(line), item_area);
    }
}
