use crate::app::PortSelectPopup;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

/// 端口选择弹窗组件
pub struct PortSelectPopupWidget {
    list_state: ListState,
}

impl PortSelectPopupWidget {
    pub fn new() -> Self {
        Self {
            list_state: ListState::default(),
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, popup: &mut PortSelectPopup) {
        if !popup.is_visible() {
            return;
        }

        // 计算弹窗大小
        let port_count = popup.len();
        if port_count == 0 {
            // 没有可用端口时显示提示
            let width = std::cmp::min(40, area.width.saturating_sub(4));
            let height = 5;
            let x = area.x + (area.width.saturating_sub(width)) / 2;
            let y = area.y + (area.height.saturating_sub(height)) / 2;
            let popup_area = Rect::new(x, y, width, height);

            let block = Block::new()
                .title(" 选择设备 ")
                .title_style(Style::new().fg(Color::Cyan))
                .borders(Borders::ALL)
                .border_style(Style::new().fg(Color::Yellow))
                .style(Style::new().bg(Color::DarkGray).fg(Color::White));

            frame.render_widget(block, popup_area);

            let content = Paragraph::new("未找到可用设备")
                .style(Style::new().fg(Color::Gray));
            frame.render_widget(
                content,
                Rect::new(popup_area.x + 1, popup_area.y + 1, width - 2, 1),
            );
            return;
        }

        let height = std::cmp::min(port_count as u16 + 2, area.height.saturating_sub(4));
        let width = std::cmp::min(40, area.width.saturating_sub(4));
        let x = area.x + (area.width.saturating_sub(width)) / 2;
        let y = area.y + (area.height.saturating_sub(height)) / 2;
        let popup_area = Rect::new(x, y, width, height);

        // 更新列表状态
        self.list_state.select(Some(popup.selected_index));

        // 创建列表项
        let items: Vec<ListItem> = popup.ports.iter().map(|port| {
            let port_type = if port.contains("USB") || port.contains("ACM") {
                " [CDC]"
            } else {
                ""
            };
            ListItem::new(Span::raw(format!("{port}{port_type}")))
        }).collect();

        // 创建块
        let block = Block::new()
            .title(" 选择设备 (↑/↓ 选择, Enter 连接, Esc 取消) ")
            .title_style(Style::new().fg(Color::Cyan))
            .borders(Borders::ALL)
            .border_style(Style::new().fg(Color::Green))
            .style(Style::new().bg(Color::DarkGray).fg(Color::White));

        frame.render_widget(block, popup_area);

        // 渲染列表
        let list = List::new(items)
            .highlight_style(Style::new().bg(Color::Blue).fg(Color::White))
            .highlight_symbol("> ");

        let content_area = Rect::new(
            popup_area.x + 1,
            popup_area.y + 1,
            popup_area.width.saturating_sub(2),
            popup_area.height.saturating_sub(2),
        );

        frame.render_stateful_widget(list, content_area, &mut self.list_state);
    }
}

impl Default for PortSelectPopupWidget {
    fn default() -> Self {
        Self::new()
    }
}
