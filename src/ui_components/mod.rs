use crate::app::Popup;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

/// 创建带标题的 Block
///
/// # Arguments
///
/// * `title` - 标题
/// * `border_color` - 边框颜色
/// * `title_color` - 标题颜色
pub fn create_block(title: String, border_color: Color, title_color: Color) -> Block<'static> {
    Block::new()
        .title(title)
        .title_style(Style::new().fg(title_color).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::new().fg(border_color))
}

/// 获取选中指示器
/// - 未选中: " "
/// - 选中: "○"
/// - 选中并编辑: "▶"
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
