use crate::app::PopupConfig;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

/// 创建带标题的 Block
#[must_use]
pub fn create_block(title: String, border_color: Color, title_color: Color) -> Block<'static> {
    Block::new()
        .title(title)
        .title_style(Style::new().fg(title_color).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::new().fg(border_color))
}

/// 获取选中指示器
#[must_use]
pub fn get_indicator(is_selected: bool, is_editing: bool) -> &'static str {
    if is_selected {
        if is_editing {
            "▶"
        } else {
            "○"
        }
    } else {
        " "
    }
}

/// 通用弹窗组件
pub struct PopupWidget;

impl PopupWidget {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, config: &PopupConfig) {
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

        // 内容区域: x+1 / y+1, 高度扣掉上下边框各 1 行
        let content_area = Rect::new(
            popup_area.x + 1,
            popup_area.y + 1,
            width.saturating_sub(2),
            height.saturating_sub(2),
        );
        let content = Paragraph::new(config.content.clone()).style(Style::new().fg(Color::White));
        frame.render_widget(content, content_area);
    }
}

impl Default for PopupWidget {
    fn default() -> Self {
        Self::new()
    }
}
