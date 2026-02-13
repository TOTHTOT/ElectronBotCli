use crate::app::MenuItem;
use crate::ui_components::create_block;
use ratatui::{
    prelude::*,
    widgets::{List, ListItem, ListState},
};

pub fn render(frame: &mut Frame, area: Rect, menu_state: &mut ListState) {
    let menu_items: Vec<ListItem> = MenuItem::all()
        .iter()
        .map(|item| ListItem::new(item.title()))
        .collect();

    let menu = List::new(menu_items)
        .highlight_style(
            Style::new()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");
    let outer_block = create_block("菜单".to_string(), Color::LightBlue, Color::LightBlue);
    let inner_area = outer_block.inner(area);
    frame.render_widget(outer_block, area);
    frame.render_stateful_widget(menu, inner_area, menu_state);
}
