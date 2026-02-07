/// 界面显示, app 模块调用
mod pages;
mod sidebar;

use crate::app::{App, MenuItem};
use ratatui::prelude::*;
use crate::ui_components::port_select_popup::PortSelectPopupWidget;

pub fn render(frame: &mut Frame, app: &mut App) {
    let chunks = Layout::new(
        Direction::Horizontal,
        [Constraint::Length(20), Constraint::Min(0)],
    )
    .split(frame.area());

    // 渲染侧边栏
    sidebar::render(frame, chunks[0], &mut app.menu_state);

    // 渲染主内容区域
    match app.selected_menu {
        MenuItem::DeviceStatus => pages::device_status::render(frame, chunks[1], app),
        MenuItem::DeviceControl => pages::device_control::render(frame, chunks[1], app),
        MenuItem::Settings => pages::settings::render(frame, chunks[1]),
        MenuItem::About => pages::about::render(frame, chunks[1]),
    }

    // 渲染端口选择弹窗
    let mut port_select_widget = PortSelectPopupWidget::new();
    port_select_widget.render(frame, frame.area(), &mut app.port_select_popup);
}
