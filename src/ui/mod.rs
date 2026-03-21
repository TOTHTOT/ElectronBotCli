pub mod pages;
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

    // 渲染侧边栏，传入焦点状态
    sidebar::render(
        frame,
        chunks[0],
        &mut app.ui.menu_state,
        app.ui.left_focused,
    );

    // 根据焦点状态选择右侧内容的边框颜色
    let right_border_color = if app.ui.left_focused {
        Color::LightBlue
    } else {
        Color::Green
    };

    match app.ui.selected_menu {
        MenuItem::DeviceStatus => {
            pages::device_status::render(frame, chunks[1], app, right_border_color)
        }
        MenuItem::DeviceControl => {
            pages::device_control::render(frame, chunks[1], app, right_border_color)
        }
        MenuItem::Settings => pages::settings::render(
            frame,
            chunks[1],
            app.ui.settings_selected,
            &app.config,
            app.ui.in_edit_settings_mode,
            &app.ui.edit_buffer,
            right_border_color,
        ),
        MenuItem::About => pages::about::render(frame, chunks[1], right_border_color),
        MenuItem::LlmTest => pages::llm_test::render(frame, chunks[1], app, right_border_color),
    }

    // 渲染弹窗
    let mut popup_widget = PopupWidget::new();
    popup_widget.render(frame, frame.area(), &mut app.popup);
}
