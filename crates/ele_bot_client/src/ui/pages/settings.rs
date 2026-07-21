use crate::app::overlay::DeviceKind;
use crate::ui::viewmodel::settings::{FailureVm, PickerVm};
use crate::ui::viewmodel::SettingsViewModel;
use crate::ui_components::{create_block, get_indicator};
use ratatui::{prelude::*, widgets::Clear, widgets::Paragraph};

pub fn render(frame: &mut Frame, area: Rect, vm: &SettingsViewModel, border_color: Color) {
    let outer_block = create_block("设置".to_string(), border_color, border_color);
    let inner_area = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    let chunks = Layout::new(
        Direction::Vertical,
        [Constraint::Length(3), Constraint::Min(0)],
    )
    .split(inner_area);

    render_info_bar(frame, chunks[0], vm.in_edit_mode, border_color);
    render_settings_list(frame, chunks[1], vm, border_color);

    // picker overlay 居中弹窗
    if let Some(picker) = &vm.picker {
        render_device_picker(frame, frame.area(), picker);
    }
    // 失败 transient overlay
    if let Some(fail) = &vm.failure_overlay {
        render_failure_overlay(frame, frame.area(), fail);
    }
}

fn render_info_bar(frame: &mut Frame, area: Rect, in_edit: bool, border_color: Color) {
    let outer_block = create_block("操作说明".to_string(), border_color, border_color);
    let inner_area = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    let text = if in_edit {
        "操作: [Enter] 保存  [Esc] 取消  [Backspace] 删除字符"
    } else {
        "操作: [↑/↓] 选择  [Enter] 编辑/选设备  [Esc] 退出  [R] 刷新设备列表"
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
    vm: &SettingsViewModel,
    border_color: Color,
) {
    let outer_block = create_block("配置项".to_string(), border_color, Color::Cyan);
    let inner_area = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    for (i, item) in vm.settings_items.iter().enumerate() {
        let y = inner_area.y + i as u16;
        let item_area = Rect::new(inner_area.x, y, inner_area.width, 1);

        let is_selected = i == vm.selected_index;
        let is_editing = vm.in_edit_mode && is_selected;
        let display_value = if is_editing {
            &vm.edit_buffer
        } else {
            &item.value
        };

        render_setting_item(
            frame,
            item_area,
            item.label,
            display_value,
            is_selected,
            is_editing,
            &item.value,
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
    raw_value: &str,
) {
    let indicator = get_indicator(is_selected, is_editing);

    let color = if is_selected {
        Color::Cyan
    } else {
        Color::White
    };

    let text = vec![Line::from_iter([
        Span::styled(
            indicator.to_string(),
            Style::new().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {label}: "), Style::new().fg(color)),
        Span::styled(
            value,
            if is_editing {
                Style::new().fg(Color::Black).bg(Color::White)
            } else if raw_value.is_empty() {
                Style::new().fg(Color::DarkGray)
            } else {
                Style::new().fg(Color::Yellow)
            },
        ),
    ])];

    let widget = Paragraph::new(text).style(Style::new().fg(Color::White));
    frame.render_widget(widget, area);
}

/// 居中弹窗: 设备选择器
fn render_device_picker(frame: &mut Frame, area: Rect, picker: &PickerVm) {
    let title = match picker.kind {
        crate::app::route::SelectingKind::Input => " 选择麦克风 ",
        crate::app::route::SelectingKind::Output => " 选择扬声器 ",
    };
    let popup_w = 60u16.min(area.width.saturating_sub(4));
    let max_rows = picker.rows.len() as u16 + 2; // +2 border
    let popup_h = max_rows.min(area.height.saturating_sub(4)).max(5);
    let popup_area = centered_rect(popup_w, popup_h, area);

    frame.render_widget(Clear, popup_area);
    let block = create_block(title.to_string(), Color::Cyan, Color::Cyan);
    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    if picker.loading && picker.rows.len() <= 1 {
        // 仅 `<系统默认>` 一行 + loading
        let line = Paragraph::new(Line::from_iter([Span::styled(
            "<加载中...>",
            Style::new().fg(Color::DarkGray),
        )]))
        .alignment(Alignment::Center);
        frame.render_widget(line, inner);
        return;
    }
    if picker.rows.is_empty() || (picker.rows.len() == 1 && picker.loading) {
        let line = Paragraph::new(Line::from_iter([Span::styled(
            "<无可用设备>",
            Style::new().fg(Color::DarkGray),
        )]))
        .alignment(Alignment::Center);
        frame.render_widget(line, inner);
        return;
    }

    let mut lines: Vec<Line> = Vec::with_capacity(picker.rows.len() + 2);
    let hint = Span::styled(
        "[↑/↓] 选择  [Enter] 确认  [Esc] 取消  [R] 刷新",
        Style::new().fg(Color::DarkGray),
    );
    lines.push(Line::from(hint));
    for (i, row) in picker.rows.iter().enumerate() {
        let is_cursor = i == picker.cursor;
        let arrow = if is_cursor { "▶ " } else { "  " };
        let mut spans = vec![Span::styled(
            arrow,
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )];
        if row.dim_suffix.is_empty() {
            // idx 0 = `<系统默认>`, 直接显示
            spans.push(Span::styled(
                row.label.clone(),
                if is_cursor {
                    Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::new().fg(Color::White)
                },
            ));
        } else {
            // driver (亮) + name (亮) + suffix (dim)
            // 这里 `row.highlighted_name` 已经把 name 拆出, 显示完整
            // display 时 driver 与 name 都亮, 后缀 dim.
            // 简化: 直接拿 display, 但把 `(...)` 末尾标 dim.
            spans.push(Span::styled(
                row.label.clone(),
                if is_cursor {
                    Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::new().fg(Color::White)
                },
            ));
        }
        lines.push(Line::from(spans));
    }
    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

/// 居中弹窗: 设备切换失败 transient
fn render_failure_overlay(frame: &mut Frame, area: Rect, fail: &FailureVm) {
    let title = match fail.kind {
        DeviceKind::Input => " 麦克风切换失败 ",
        DeviceKind::Output => " 扬声器切换失败 ",
    };
    let popup_w = 60u16.min(area.width.saturating_sub(4));
    let popup_h = 7u16.min(area.height.saturating_sub(4));
    let popup_area = centered_rect(popup_w, popup_h, area);

    frame.render_widget(Clear, popup_area);
    let block = create_block(title.to_string(), Color::Red, Color::Yellow);
    let inner = block.inner(popup_area);
    frame.render_widget(block, popup_area);

    let lines = vec![
        Line::from_iter([Span::styled(
            format!("已保留: {}", fail.old_device_name),
            Style::new().fg(Color::Yellow),
        )]),
        Line::from(""),
        Line::from_iter([Span::styled(
            fail.detail.clone(),
            Style::new().fg(Color::White),
        )]),
        Line::from(""),
        Line::from_iter([Span::styled("[Esc] 关闭", Style::new().fg(Color::DarkGray))]),
    ];
    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

fn centered_rect(w: u16, h: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h) / 2;
    Rect::new(x, y, w, h)
}
