mod pages;
mod sidebar;

use crate::app::{App, MenuItem};
use crate::ui_components::PopupWidget;
use ratatui::prelude::*;

pub fn render(frame: &mut Frame, app: &mut App) {
    let chunks = Layout::new(
        Direction::Horizontal,
        [Constraint::Length(20), Constraint::Min(0)],
    )
    .split(frame.area());

    sidebar::render(frame, chunks[0], &mut app.menu_state);

    match app.selected_menu {
        MenuItem::DeviceStatus => pages::device_status::render(frame, chunks[1], app),
        MenuItem::DeviceControl => pages::device_control::render(frame, chunks[1], app),
        MenuItem::Settings => pages::settings::render(frame, chunks[1]),
        MenuItem::About => pages::about::render(frame, chunks[1]),
    }

    // 渲染弹窗
    let mut popup_widget = PopupWidget::new();
    popup_widget.render(frame, frame.area(), &mut app.popup);
}
