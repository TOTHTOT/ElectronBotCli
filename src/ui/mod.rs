pub mod pages;
mod sidebar;
mod viewmodel;

use crate::app::{App, MenuItem};
use crate::ui::viewmodel::{
    DeviceControlViewModel, DeviceStatusViewModel, LlmTestViewModel, SettingsViewModel,
    TtsTestViewModel,
};
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

    // 创建 ViewModel, 每一帧都会重新创建, 这样值就是最新的,
    // 通过 ViewModel 让 ui 渲染基本摆脱了 app.
    let status_vm = DeviceStatusViewModel::from_app(app);
    let control_vm = DeviceControlViewModel::from_app(app);
    let llm_vm = LlmTestViewModel::from_app(app);
    let settings_vm = SettingsViewModel::from_app(app);
    let tts_vm = TtsTestViewModel::from_app(app);

    match app.ui.selected_menu {
        MenuItem::DeviceStatus => {
            pages::device_status::render(frame, chunks[1], &status_vm, right_border_color)
        }
        MenuItem::DeviceControl => {
            pages::device_control::render(frame, chunks[1], &control_vm, right_border_color)
        }
        MenuItem::Settings => pages::settings::render(
            frame,
            chunks[1],
            settings_vm.selected_index,
            &app.config,
            settings_vm.in_edit_mode,
            &settings_vm.edit_buffer,
            right_border_color,
            settings_vm.in_device_selection_mode,
            settings_vm.device_selection_index,
            &settings_vm.input_devices,
            &settings_vm.output_devices,
        ),
        MenuItem::About => pages::about::render(frame, chunks[1], right_border_color),
        MenuItem::LlmTest => pages::llm_test::render(frame, chunks[1], &llm_vm, right_border_color),
        MenuItem::TtsTest => pages::tts_test::render(frame, chunks[1], &tts_vm, right_border_color),
    }

    // 渲染弹窗
    let mut popup_widget = PopupWidget::new();
    popup_widget.render(frame, frame.area(), &mut app.popup);
}
