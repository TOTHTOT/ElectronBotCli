use crate::app::MenuItem;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, ListState},
};

pub fn render(frame: &mut Frame, area: Rect, menu_state: &mut ListState) {
    let menu_items: Vec<ListItem> = MenuItem::all()
        .iter()
        .map(|item| ListItem::new(item.title()))
        .collect();

    let menu = List::new(menu_items)
        .block(
            Block::new()
                .title("菜单")
                .borders(Borders::ALL)
                .border_style(Style::new().fg(Color::Cyan)),
        )
        .highlight_style(
            Style::new()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    frame.render_stateful_widget(menu, area, menu_state);
}
