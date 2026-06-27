pub mod pages;
mod sidebar;
mod viewmodel;

use crate::app::route::DeviceControlMode;
use crate::app::{App, MenuItem, Overlay, PopupConfig, Route};
use crate::ui::viewmodel::{DeviceControlViewModel, SettingsViewModel};
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

    // DeviceControl VM 含 UI 特定常量(servo names/ranges)与派生字段, 每帧重建
    let mut control_vm = DeviceControlViewModel::from_app(app);
    control_vm.is_servo_mode = matches!(
        app.ui.mode.route,
        Route::DeviceControl {
            mode: DeviceControlMode::Active
        }
    );
    let settings_vm = SettingsViewModel::from_app(app);

    // Route 驱动页面
    match &app.ui.mode.route {
        Route::Nav { .. } => {
            // Nav 状态: 仍渲染 last_entered 对应的页面(保留旧行为)
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
                &control_vm,
                &settings_vm,
                last,
                right_border_color,
            );
        }
        Route::DeviceControl { .. } => {
            pages::device_control::render(frame, chunks[1], &control_vm, right_border_color)
        }
        Route::Settings { .. } => {
            pages::settings::render(frame, chunks[1], &settings_vm, right_border_color)
        }
        Route::LlmTest => pages::llm_test::render(
            frame,
            chunks[1],
            &app.ai.llm_test_state,
            app.ai
                .is_processing
                .load(std::sync::atomic::Ordering::Relaxed),
            right_border_color,
        ),
        Route::TtsTest => {
            pages::tts_test::render(frame, chunks[1], &app.ai.tts_test_state, right_border_color)
        }
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

fn render_menu_item_page(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    control_vm: &DeviceControlViewModel,
    settings_vm: &SettingsViewModel,
    item: MenuItem,
    border_color: Color,
) {
    match item {
        MenuItem::DeviceStatus => pages::device_status::render(frame, area, app, border_color),
        MenuItem::DeviceControl => {
            pages::device_control::render(frame, area, control_vm, border_color)
        }
        MenuItem::LlmTest => pages::llm_test::render(
            frame,
            area,
            &app.ai.llm_test_state,
            app.ai
                .is_processing
                .load(std::sync::atomic::Ordering::Relaxed),
            border_color,
        ),
        MenuItem::TtsTest => {
            pages::tts_test::render(frame, area, &app.ai.tts_test_state, border_color)
        }
        MenuItem::Settings => pages::settings::render(frame, area, settings_vm, border_color),
        MenuItem::About => pages::about::render(frame, area, border_color),
    }
}
