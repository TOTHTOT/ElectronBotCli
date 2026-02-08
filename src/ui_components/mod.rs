use crate::app::CommPopup;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

/// 通信提示弹窗组件
impl CommPopupWidget {
    pub fn new() -> Self {
        Self
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, popup: &mut CommPopup) {
        if !popup.is_visible() {
            return;
        }

        let width = std::cmp::min(40, area.width.saturating_sub(4));
        let height = 5;
        let x = area.x + (area.width.saturating_sub(width)) / 2;
        let y = area.y + (area.height.saturating_sub(height)) / 2;
        let popup_area = Rect::new(x, y, width, height);

        let block = Block::new()
            .title(" 连接设备 ")
            .title_style(Style::new().fg(Color::Cyan))
            .borders(Borders::ALL)
            .border_style(Style::new().fg(Color::Green))
            .style(Style::new().bg(Color::DarkGray).fg(Color::White));

        frame.render_widget(block, popup_area);

        let content =
            Paragraph::new("正在通过 USB 连接设备...").style(Style::new().fg(Color::White));
        frame.render_widget(
            content,
            Rect::new(popup_area.x + 1, popup_area.y + 2, width - 2, 1),
        );
    }
}

pub struct CommPopupWidget;

impl Default for CommPopupWidget {
    fn default() -> Self {
        Self::new()
    }
}
