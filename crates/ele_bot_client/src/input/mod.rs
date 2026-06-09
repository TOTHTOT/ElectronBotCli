//! 事件模块 - 按 AppMode 分发按键输入
//!
//! 调度优先级:
//! 1. 全局热键 (Esc 退到 Nav 或退出, Ctrl+C 退出)
//! 2. 当前 overlay (若 Some) - overlay 优先消费按键
//! 3. 当前 route 的 per-route handler
//!
//! 关键设计: dispatch 用 `matches!` 而不是 `match &mut ...`, 所以路由的
//! 借用只在每个 handler 的小作用域内持有, 不会跨方法调用冲突。这样
//! 任何 handler 修改 `app.ui.mode.route` 都不会被覆盖。

mod device;
mod llm_test;
mod menu;
mod settings;
mod tts_test;

pub use device::DeviceEvent;
pub use menu::MenuEvent;
pub use settings::SettingsEvent;

use crate::app::overlay::PopupDismiss;
use crate::app::route::DeviceControlMode;
use crate::app::{App, MenuItem, Overlay, Route};
use crate::input::llm_test::handle as handle_llm_test;
use crate::input::tts_test::handle as handle_tts_test;
use crossterm::event::{KeyCode, KeyModifiers};

/// 应用事件 — 派发到 App 上的高层语义操作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppEvent {
    Common(CommonEvent),
    Menu(MenuEvent),
    Device(DeviceEvent),
    Settings(SettingsEvent),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommonEvent {
    Quit,
    ConfirmQuit,
    None,
}

impl From<CommonEvent> for AppEvent {
    fn from(e: CommonEvent) -> Self {
        AppEvent::Common(e)
    }
}
impl From<MenuEvent> for AppEvent {
    fn from(e: MenuEvent) -> Self {
        AppEvent::Menu(e)
    }
}
impl From<DeviceEvent> for AppEvent {
    fn from(e: DeviceEvent) -> Self {
        AppEvent::Device(e)
    }
}
impl From<SettingsEvent> for AppEvent {
    fn from(e: SettingsEvent) -> Self {
        AppEvent::Settings(e)
    }
}

pub fn handle_event(app: &mut App, event: AppEvent) {
    match event {
        AppEvent::Common(CommonEvent::Quit) => app.quit(),
        AppEvent::Common(CommonEvent::ConfirmQuit) => app.confirm_quit(),
        AppEvent::Common(CommonEvent::None) => {}
        AppEvent::Menu(e) => menu::handle(app, e),
        AppEvent::Device(e) => device::handle(app, e),
        AppEvent::Settings(e) => settings::handle(app, e),
    }
}

/// 按 AppMode 分发按键输入
pub fn handle_by_mode(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    // (0) 全局热键 (Esc / Ctrl+C 跨所有 mode)
    if code == KeyCode::Esc {
        if app.ui.mode.overlay.is_some() {
            handle_overlay(app, code);
            return;
        }
        match &app.ui.mode.route {
            Route::Nav { .. } | Route::About => {
                // 弹"确认退出", 等用户 Enter 确认 / Esc 取消
                app.confirm_quit();
                return;
            }
            _ => {
                app.back_to_nav();
                return;
            }
        }
    }
    if modifiers.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('c') {
        // Ctrl+C 是"硬退出"逃生口, 不弹确认
        app.quit();
        return;
    }

    // (1) Overlay 优先
    if app.ui.mode.overlay.is_some() {
        handle_overlay(app, code);
        return;
    }

    // (2) Route - 用 matches! 分发 (不持路由的借用)
    if matches!(app.ui.mode.route, Route::Nav { .. }) {
        handle_nav(app, code);
    } else if matches!(app.ui.mode.route, Route::DeviceControl { .. }) {
        handle_device_control(app, code);
    } else if matches!(app.ui.mode.route, Route::Settings { .. }) {
        handle_settings(app, code);
    } else if matches!(app.ui.mode.route, Route::LlmTest) {
        handle_llm_test_route(app, code);
    } else if matches!(app.ui.mode.route, Route::TtsTest) {
        handle_tts_test_route(app, code);
    } else if matches!(app.ui.mode.route, Route::About) {
        handle_about(app, code);
    }
}

/// 侧边栏(导航)模式: Up/Down 选菜单, Enter 进入页面
fn handle_nav(app: &mut App, code: KeyCode) {
    let last_entered = match &app.ui.mode.route {
        Route::Nav { last_entered } => *last_entered,
        _ => return,
    };
    match code {
        KeyCode::Up => app.prev_menu(),
        KeyCode::Down => app.next_menu(),
        KeyCode::Enter => {
            let last = app
                .ui
                .menu_state
                .selected()
                .and_then(|i| MenuItem::all().get(i).copied())
                .unwrap_or(last_entered);
            app.ui.mode.route = Route::from(last);
        }
        _ => {}
    }
}

/// DeviceControl 页面
///
/// - Idle: Up/Down 切菜单项, Enter 切到 Active(进入舵机控制)
/// - Active: 方向键调舵机, Esc/Enter 退到 Idle
fn handle_device_control(app: &mut App, code: KeyCode) {
    let mode = match &app.ui.mode.route {
        Route::DeviceControl { mode } => *mode,
        _ => return,
    };
    if mode == DeviceControlMode::Idle {
        match code {
            KeyCode::Up => app.prev_menu(),
            KeyCode::Down => app.next_menu(),
            KeyCode::Enter => {
                if let Route::DeviceControl { mode } = &mut app.ui.mode.route {
                    *mode = DeviceControlMode::Active;
                }
            }
            _ => {}
        }
        return;
    }
    // Active
    match code {
        KeyCode::Enter => {
            if let Route::DeviceControl { mode } = &mut app.ui.mode.route {
                *mode = DeviceControlMode::Idle;
            }
        }
        KeyCode::Up => app.prev_servo(),
        KeyCode::Down => app.next_servo(),
        KeyCode::Left => app.decrease_selected(),
        KeyCode::Right => app.increase_selected(),
        KeyCode::Char('s') | KeyCode::Char('S') => app.take_screenshot(),
        KeyCode::Char('f') | KeyCode::Char('F') => app.toggle_face_tracking(),
        _ => {}
    }
}

/// Settings 页面
fn handle_settings(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Enter => {
            app.begin_settings_edit();
            if let Route::Settings { editing, .. } = &app.ui.mode.route {
                if let Some(field) = editing.clone() {
                    app.ui.mode.overlay = Some(Overlay::EditField(field));
                }
            }
        }
        KeyCode::Up => app.settings_prev(),
        KeyCode::Down => app.settings_next(),
        _ => {}
    }
}

fn handle_llm_test_route(app: &mut App, code: KeyCode) {
    handle_llm_test(app, code);
}

fn handle_tts_test_route(app: &mut App, code: KeyCode) {
    handle_tts_test(app, code);
}

fn handle_about(_app: &mut App, _code: KeyCode) {}

/// 模态层: overlay 优先消费按键
fn handle_overlay(app: &mut App, code: KeyCode) {
    let overlay = app.ui.mode.overlay.take();
    match overlay {
        Some(Overlay::EditField(field)) => {
            let new_overlay = match code {
                KeyCode::Esc => None,
                KeyCode::Enter => {
                    if let Route::Settings { editing, .. } = &mut app.ui.mode.route {
                        *editing = Some(field);
                    }
                    app.commit_settings_edit();
                    if let Route::Settings { editing, .. } = &mut app.ui.mode.route {
                        *editing = None;
                    }
                    None
                }
                KeyCode::Backspace => {
                    let mut f = field;
                    f.buffer.pop();
                    if let Route::Settings { editing, .. } = &mut app.ui.mode.route {
                        *editing = Some(f.clone());
                    }
                    Some(Overlay::EditField(f))
                }
                KeyCode::Char(c) => {
                    let mut f = field;
                    f.buffer.push(c);
                    if let Route::Settings { editing, .. } = &mut app.ui.mode.route {
                        *editing = Some(f.clone());
                    }
                    Some(Overlay::EditField(f))
                }
                _ => Some(Overlay::EditField(field)),
            };
            app.ui.mode.overlay = new_overlay;
        }
        Some(Overlay::Popup { on_dismiss, config }) => match (code, on_dismiss) {
            (KeyCode::Esc, _) => {
                match on_dismiss {
                    PopupDismiss::Cancel => {}
                    PopupDismiss::CancelConnect => {
                        app.stop_comm_thread();
                    }
                    PopupDismiss::ConfirmQuit => {}
                }
                // 弹窗关闭 (take 已把 overlay 置为 None)
            }
            (KeyCode::Enter, PopupDismiss::ConfirmQuit) => {
                // 确认退出
                app.quit();
            }
            (KeyCode::Enter, _) => {
                // 其它变体不支持 Enter 确认, 弹窗保持
                app.ui.mode.overlay = Some(Overlay::Popup { config, on_dismiss });
            }
            _ => {
                app.ui.mode.overlay = Some(Overlay::Popup { config, on_dismiss });
            }
        },
        None => unreachable!("guarded by caller"),
    }
}
