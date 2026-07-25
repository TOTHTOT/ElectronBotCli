use crate::app::overlay::DeviceKind;
use crate::ui::viewmodel::settings::{FailureVm, PickerVm};
use crate::ui::viewmodel::SettingsViewModel;
use crate::ui_components::{create_block, get_indicator};
use ratatui::{prelude::*, widgets::Clear, widgets::Paragraph};
use unicode_width::UnicodeWidthStr;

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
        // 编辑态: 列出全部支持的按键. 中文混排, 终端窄 (<80 列) 时
        // Paragraph 默认会截断, 这里不加 wrap=Word; 用户实际使用的
        // 终端多在 100 列以上, 80 列以下截断不致命.
        "操作: [Enter] 保存  [Esc] 取消  [Backspace] 删前  [Delete] 删后  [←→] 移动  [Home/End] 跳首尾"
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
        // caret 字符索引; 仅当 is_editing 时该值有意义, 其它行用 0.
        let caret_char_idx = if is_editing { vm.edit_cursor } else { 0 };

        render_setting_item(
            frame,
            item_area,
            item.label,
            display_value,
            caret_char_idx,
            is_selected,
            is_editing,
            &item.value,
        );
    }
}

/// 渲染设置项
///
/// # caret 渲染策略
///
/// 编辑态 (`is_editing == true`) 把 buffer 拆三段:
///
/// ```text
/// <indicator> <label>: <before>█<after>
///                   ────────反色高亮, █ 是反色块字符作为 caret
/// ```
///
/// **不**用 `Frame::set_cursor_position`: `crate::ui::mod.rs::render`
/// 的 popup layer 在 `EditField` 上方 `Clear`, 终端原生光标位置会被
/// Clear 抹掉 — 出现"按键时看见光标动, 静下来位置又不对"的诡异 bug.
/// 块字符 caret 对 overlay 渲染兼容, 代价是不能闪烁.
///
/// 字符切分严格按 `char` 走 (`chars().take(caret_char_idx)`),
/// `caret_char_idx` 单位是 char 不是 byte — `EditField::before_cursor`
/// 提供等价行为, 本函数为渲染独立计算避免把 `EditField` 类型漏到 UI 层.
///
/// 参数表偏长 (8 个) 是因为:
/// 1. ratatui 的 `frame` + `area` 必须按值传 (生命周期约束)
/// 2. `label` / `value` / `raw_value` 是三种不同语义的文本 (`value` 是
///    编辑态 buffer, `raw_value` 是原始显示用, 用于判定"空值占位色")
/// 3. `is_selected` / `is_editing` 互不蕴含 (选中不等于编辑)
/// 4. `caret_char_idx` 仅在 `is_editing` 时有效
///
/// 合成 struct 反而把 helper 变成一个轻量 builder, 不符合"渲染一行就地调"
/// 的本意 — 这里 `#[allow]` 而不是合并.
#[allow(clippy::too_many_arguments)]
fn render_setting_item(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    value: &str,
    caret_char_idx: usize,
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

    let base_style = Style::new().fg(color);
    let indicator_span = Span::styled(
        indicator.to_string(),
        base_style.add_modifier(Modifier::BOLD),
    );
    let label_span = Span::styled(format!(" {label}: "), base_style);

    let text = if is_editing {
        // 三段拼接: before + caret(块字符) + after
        let before: String = value.chars().take(caret_char_idx).collect();
        let after: String = value.chars().skip(caret_char_idx).collect();
        let ed_style = Style::new().fg(Color::Black).bg(Color::White);
        // caret 用 ASCII 块字符 `\u{2588}` 占一列; 同 bg 区分普通 fg.
        let caret_span = Span::styled("\u{2588}", ed_style);
        vec![Line::from_iter([
            indicator_span,
            label_span,
            Span::styled(before, ed_style),
            caret_span,
            Span::styled(after, ed_style),
        ])]
    } else {
        let val_style = if raw_value.is_empty() {
            // 占位色 — 不在 UI 上把"空"显示成普通黄字
            Style::new().fg(Color::DarkGray)
        } else {
            Style::new().fg(Color::Yellow)
        };
        vec![Line::from_iter([
            indicator_span,
            label_span,
            Span::styled(value, val_style),
        ])]
    };

    let widget = Paragraph::new(text).style(Style::new().fg(Color::White));
    frame.render_widget(widget, area);
}

/// 居中弹窗: 设备选择器
///
/// 宽高按内容自适应:
/// - 宽度 = `最长行 display width + 光标占位 2 + 内边距 2`, 再 clamp 到
///   `area.width - 4` 留左右空隙, 仍超宽时按可视宽截断 (不期待真实场景
///   长于此, 终端 < 30 列极少).
/// - 高度 = rows 总数 + 提示行 1 + border 2 + 内边距 2, clamp 到
///   `area.height - 4`. 超高时仅显示 cursor 居中的窗口, 滚动窗口随 cursor
///   移动 — 让用户始终能看到当前选中的行.
fn render_device_picker(frame: &mut Frame, area: Rect, picker: &PickerVm) {
    let title = match picker.kind {
        crate::app::route::SelectingKind::Input => " 选择麦克风 ",
        crate::app::route::SelectingKind::Output => " 选择扬声器 ",
        crate::app::route::SelectingKind::Camera => " 选择摄像头 ",
    };

    // 宽度: 最长行的可见字符数 + 2(cursor 箭头) + 2(边距) + 2(border).
    let mut max_label_w: u16 = 0;
    for row in &picker.rows {
        let w = UnicodeWidthStr::width(row.label.as_str()) as u16;
        if w > max_label_w {
            max_label_w = w;
        }
    }
    let needed_w = (max_label_w + 6).max(title_width(title));
    let max_w = area.width.saturating_sub(2).max(8); // 至少留 2 列空隙
    let popup_w = needed_w.min(max_w).max(10);

    // 高度: 顶部 hint + rows + border + 上下边距. rows 远超可视时按
    // 可视高度 - 6 限, 然后用 cursor-centered 滚动窗口.
    let content_rows_needed = picker.rows.len() as u16 + 1; // +1 hint
    let ideal_h = content_rows_needed + 4; // +2 border +2 padding
    let max_h = area.height.saturating_sub(2).max(5);
    let popup_h = ideal_h.min(max_h).max(5);

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

    let inner_h = inner.height as usize;
    // inner 减去 hint(1) = rows 可用行数, 至少 1 行
    let rows_visible = inner_h.saturating_sub(1).max(1);
    let total_rows = picker.rows.len();
    let cursor = picker.cursor.min(total_rows.saturating_sub(1));
    // 滚动窗口: 围绕 cursor 居中. 窗口起点 = max(0, cursor - rows_visible/2),
    // 然后 clamp 到 total_rows - rows_visible.
    let window_start = if total_rows <= rows_visible {
        0
    } else {
        let half = rows_visible / 2;
        let mut start = cursor as i32 - half as i32;
        let max_start = total_rows as i32 - rows_visible as i32;
        if start < 0 {
            start = 0;
        }
        if start > max_start {
            start = max_start;
        }
        start as usize
    };
    let window_end = (window_start + rows_visible).min(total_rows);

    let mut lines: Vec<Line> = Vec::with_capacity(rows_visible + 1);
    let hint = Span::styled(
        "[↑/↓] 选择  [Enter] 确认  [Esc] 取消  [R] 刷新",
        Style::new().fg(Color::DarkGray),
    );
    lines.push(Line::from(hint));

    for idx in window_start..window_end {
        let row = &picker.rows[idx];
        let is_cursor = idx == cursor;
        let arrow = if is_cursor { "▶ " } else { "  " };
        let label_style = if is_cursor {
            Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(Color::White)
        };
        lines.push(Line::from_iter([
            Span::styled(
                arrow,
                Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
            Span::styled(row.label.clone(), label_style),
        ]));
    }
    // rows 还多但窗口放不下时, 不画底部指示行 (会挤掉 cursor), 用户滚到底自
    // 然看得见 — 跟 `enter_device_picker` 的 cursor auto-align 配套.

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

/// 计算 title 的可见字符宽 (含两侧空格), 给 popup 最小宽度参考.
fn title_width(title: &str) -> u16 {
    UnicodeWidthStr::width(title) as u16 + 4 // +4 padding
}

/// 居中弹窗: 设备切换失败 transient
fn render_failure_overlay(frame: &mut Frame, area: Rect, fail: &FailureVm) {
    let title = match fail.kind {
        DeviceKind::Input => " 麦克风切换失败 ",
        DeviceKind::Output => " 扬声器切换失败 ",
        DeviceKind::Camera => " 摄像头切换失败 ",
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
