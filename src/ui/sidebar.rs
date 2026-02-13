use crate::app::MenuItem;
use crate::ui_components::create_block;
use ratatui::{
    prelude::*,
    widgets::{List, ListItem, ListState},
};

pub fn render(frame: &mut Frame, area: Rect, menu_state: &mut ListState, left_focused: bool) {
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

    // 根据焦点状态选择边框颜色：侧边栏有焦点为绿色，否则为蓝色
    let border_color = if left_focused {
        Color::Green
    } else {
        Color::LightBlue
    };
    let outer_block = create_block("菜单".to_string(), border_color, border_color);
    let inner_area = outer_block.inner(area);
    frame.render_widget(outer_block, area);
    frame.render_stateful_widget(menu, inner_area, menu_state);
}
