pub mod pages;
mod sidebar;
mod viewmodel;

use crate::app::route::DeviceControlMode;
use crate::app::{App, MenuItem, Overlay, PopupConfig, Route};
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

    // 侧边栏焦点从 AppMode 推导
    let sidebar_focused = app.ui.mode.sidebar_focused();
    sidebar::render(frame, chunks[0], &mut app.ui.menu_state, sidebar_focused);

    let right_border_color = if sidebar_focused {
        Color::LightBlue
    } else {
        Color::Green
    };

    // 每帧重新构建 VM, 让 UI 与状态解耦
    let status_vm = DeviceStatusViewModel::from_app(app);
    let mut control_vm = DeviceControlViewModel::from_app(app);
    // 修死代码: is_servo_mode 从 Route::DeviceControl { mode } 派生
    control_vm.is_servo_mode = matches!(
        app.ui.mode.route,
        Route::DeviceControl { mode: DeviceControlMode::Active }
    );
    let llm_vm = LlmTestViewModel::from_app(app);
    let settings_vm = SettingsViewModel::from_app(app);
    let tts_vm = TtsTestViewModel::from_app(app);

    // Route 驱动页面
    match &app.ui.mode.route {
        Route::Nav { .. } => {
            // Nav 状态: 仍渲染 last_entered 对应的页面(保留旧行为)
            // 读 menu_state 第一个选项作为回退
            let last = app
                .ui
                .menu_state
                .selected()
                .and_then(|i| MenuItem::all().get(i).copied())
                .unwrap_or(MenuItem::DeviceStatus);
            render_menu_item_page(
                frame,
                chunks[1],
                app,
                &status_vm,
                &control_vm,
                &llm_vm,
                &settings_vm,
                &tts_vm,
                last,
                right_border_color,
            );
        }
        Route::DeviceControl { .. } => {
            pages::device_control::render(frame, chunks[1], &control_vm, right_border_color)
        }
        Route::Settings { .. } => {
            let (idx, in_edit, buf) = match &app.ui.mode.route {
                Route::Settings {
                    selected: _,
                    editing: Some(f),
                } => (f.index, true, f.buffer.as_str()),
                Route::Settings { selected, .. } => (*selected, false, ""),
                _ => (0, false, ""),
            };
            pages::settings::render(
                frame,
                chunks[1],
                idx,
                &app.config,
                in_edit,
                buf,
                right_border_color,
            )
        }
        Route::LlmTest => pages::llm_test::render(frame, chunks[1], &llm_vm, right_border_color),
        Route::TtsTest => pages::tts_test::render(frame, chunks[1], &tts_vm, right_border_color),
        Route::About => pages::about::render(frame, chunks[1], right_border_color),
    }

    // 弹窗(Overlay::Popup) — 顶层最后绘制
    if let Some(Overlay::Popup { config, .. }) = &app.ui.mode.overlay {
        render_popup(frame, config.clone());
    }
    // 注: EditField overlay 暂由 Settings 页面内联渲染(因为 Settings 页面
    // 已经接收 in_edit/buf 参数, 不需要单独 widget)
}

fn render_popup(frame: &mut Frame, config: PopupConfig) {
    let mut pw = PopupWidget::new();
    pw.render(frame, frame.area(), &config);
}

#[allow(clippy::too_many_arguments)]
fn render_menu_item_page(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    status_vm: &DeviceStatusViewModel,
    control_vm: &DeviceControlViewModel,
    llm_vm: &LlmTestViewModel,
    settings_vm: &SettingsViewModel,
    tts_vm: &TtsTestViewModel,
    item: MenuItem,
    border_color: Color,
) {
    match item {
        MenuItem::DeviceStatus => {
            pages::device_status::render(frame, area, status_vm, border_color)
        }
        MenuItem::DeviceControl => {
            pages::device_control::render(frame, area, control_vm, border_color)
        }
        MenuItem::LlmTest => pages::llm_test::render(frame, area, llm_vm, border_color),
        MenuItem::TtsTest => pages::tts_test::render(frame, area, tts_vm, border_color),
        MenuItem::Settings => {
            // Nav 状态: 渲染 Settings 但用 menu_state 同步的 selected
            // (与原行为一致)
            let _ = app; // 暂时不传 app
            let idx = settings_vm.selected_index;
            pages::settings::render(
                frame,
                area,
                idx,
                &app.config,
                settings_vm.in_edit_mode,
                &settings_vm.edit_buffer,
                border_color,
            )
        }
        MenuItem::About => pages::about::render(frame, area, border_color),
    }
}
