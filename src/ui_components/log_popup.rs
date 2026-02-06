use crate::app::LogQueue;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Scrollbar},
};

/// 日志 popup 组件
pub struct LogPopup {
    visible: bool,
    scroll_offset: usize,
}

#[allow(dead_code)]
impl LogPopup {
    pub fn new() -> Self {
        Self {
            visible: false,
            scroll_offset: 0,
        }
    }

    pub fn show(&mut self) {
        self.visible = true;
        self.scroll_offset = 0;
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    pub fn toggle(&mut self) {
        if self.visible {
            self.hide();
        } else {
            self.show();
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn scroll_down(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_add(1);
    }

    pub fn scroll_up(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, log_queue: &mut LogQueue) {
        // 只有 warning/error 日志才自动显示
        if !self.visible && log_queue.has_new_important() {
            self.show();
            log_queue.clear_important_flag();
        }

        if !self.visible {
            return;
        }

        let entries = log_queue.entries();
        if entries.is_empty() {
            return;
        }

        // 计算 popup 大小
        let max_height = std::cmp::min(15, area.height.saturating_sub(4));
        let max_width = std::cmp::min(80, area.width.saturating_sub(4));

        let popup_area = Rect::new(area.x + 2, area.y + 2, max_width, max_height);

        // 创建日志文本 - 只显示最新的5条
        let max_entries = std::cmp::min(5, max_height as usize - 2);
        let log_text: Vec<Line> = entries
            .iter()
            .rev() // 反转，只显示最新的
            .take(max_entries)
            .rev() // 恢复顺序
            .map(|entry| {
                Line::from(vec![
                    Span::styled(
                        entry.level.prefix(),
                        Style::new().fg(entry.level.color()).bold(),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        format!("[{}]", entry.timestamp),
                        Style::new().fg(Color::Gray),
                    ),
                    Span::raw(" "),
                    Span::styled(entry.with_count(), Style::new().fg(Color::White)),
                ])
            })
            .collect();

        // 渲染 popup 背景
        let block = Block::new()
            .title(" 日志 (按 'l' 关闭) ")
            .title_style(Style::new().fg(Color::Cyan))
            .borders(Borders::ALL)
            .border_style(Style::new().fg(Color::Magenta))
            .style(Style::new().bg(Color::DarkGray).fg(Color::White));

        frame.render_widget(block, popup_area);

        // 渲染日志内容
        let content_area = Rect::new(
            popup_area.x + 1,
            popup_area.y + 1,
            popup_area.width.saturating_sub(2),
            popup_area.height.saturating_sub(2),
        );

        let log_widget = Paragraph::new(log_text).scroll((self.scroll_offset as u16, 0));

        frame.render_widget(log_widget, content_area);

        // 如果需要滚动条
        if entries.len() > max_height as usize - 2 {
            let scrollbar = Scrollbar::new(ratatui::widgets::ScrollbarOrientation::VerticalRight)
                .thumb_style(Style::new().fg(Color::Gray))
                .track_style(Style::new().fg(Color::DarkGray));

            let scrollbar_area = Rect::new(
                popup_area.x + popup_area.width - 1,
                popup_area.y + 1,
                1,
                max_height - 2,
            );

            frame.render_stateful_widget(
                scrollbar,
                scrollbar_area,
                &mut ratatui::widgets::ScrollbarState::new(entries.len())
                    .position(self.scroll_offset),
            );
        }
    }
}

impl Default for LogPopup {
    fn default() -> Self {
        Self::new()
    }
}
