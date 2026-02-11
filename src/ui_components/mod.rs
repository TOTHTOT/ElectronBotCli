use crate::app::Popup;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

/// 通用弹窗组件
pub struct PopupWidget;

impl PopupWidget {
    pub fn new() -> Self {
        Self
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, popup: &mut Popup) {
        if !popup.is_visible() {
            return;
        }

        let config = &popup.config;
        let width = std::cmp::min(config.width, area.width.saturating_sub(4));
        let height = config.height;
        let x = area.x + (area.width.saturating_sub(width)) / 2;
        let y = area.y + (area.height.saturating_sub(height)) / 2;
        let popup_area = Rect::new(x, y, width, height);

        let block = Block::new()
            .title(config.title.clone())
            .title_style(Style::new().fg(config.title_color))
            .borders(Borders::ALL)
            .border_style(Style::new().fg(config.border_color))
            .style(Style::new().bg(config.bg_color).fg(Color::White));

        frame.render_widget(block, popup_area);

        let content = Paragraph::new(config.content.clone()).style(Style::new().fg(Color::White));
        frame.render_widget(
            content,
            Rect::new(popup_area.x + 1, popup_area.y + 2, width - 2, 1),
        );
    }
}

impl Default for PopupWidget {
    fn default() -> Self {
        Self::new()
    }
}
