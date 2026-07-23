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
use crate::app::route::{DeviceControlMode, SelectingKind};
use crate::app::{
    App, EditField, MenuItem, Overlay, Route, SETTINGS_IDX_OUTPUT, SETTINGS_IDX_SPEECH,
};
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

/// 应用事件入口 — 把语义化 [`AppEvent`] 分发给对应的子系统 handler.
///
/// 这是输入层 (`handle_nav` / `handle_device_control` / `handle_settings`)
/// 唯一允许触达子系统实现的途径。所有"按键 → 动作"的翻译都在 route
/// handler 里完成, 这里只做 dispatch —— 不读 route, 不读 overlay.
///
/// 子系统 handler:
/// - [`CommonEvent`] → 直接调 `App` 方法 (Quit / ConfirmQuit)
/// - [`MenuEvent`]   → [`menu::handle`]   (Nav 列表导航 + 设备连接)
/// - [`DeviceEvent`] → [`device::handle`] (DeviceControl::Active 舵机控制)
/// - [`SettingsEvent`] → [`settings::handle`] (Settings 列表导航 + 进入编辑)
///
/// # 何时调用
///
/// 1. **路由层** (`handle_nav` 等): 按键翻译成事件后调用本函数.
/// 2. **未来扩展**: 其它入口 (网络回调、定时器、热键重映射) 也应构造
///    `AppEvent` 并走这里, 而不是直接调 `App` 方法.
///
/// # Examples
///
/// ```rust,ignore
/// // DeviceStatus 菜单项按 Enter 时:
/// handle_event(app, AppEvent::Menu(MenuEvent::ConnectDevice));
///
/// // DeviceControl::Active 按方向键调舵机:
/// handle_event(app, AppEvent::Device(DeviceEvent::Increase));
///
/// // Settings 列表按 Enter 进入编辑:
/// handle_event(app, AppEvent::Settings(SettingsEvent::Enter));
/// ```
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
    log::info!("ui state: {:?}handling event: {:?}", app.ui, code);
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
    let event = match code {
        KeyCode::Up => Some(AppEvent::Menu(MenuEvent::Up)),
        KeyCode::Down => Some(AppEvent::Menu(MenuEvent::Down)),
        KeyCode::Enter => {
            let last = app
                .ui
                .menu_state
                .selected()
                .and_then(|i| MenuItem::all().get(i).copied())
                .unwrap_or(last_entered);
            log::info!("nav selected: {:?}", last);

            // 设备状态页: Enter = 连接/断开, 不进入新 Route
            // (Route::from(DeviceStatus) 会映射回 Route::Nav, 形成死循环)
            if last == MenuItem::DeviceStatus {
                if app.is_connected() {
                    app.stop_comm_thread();
                } else {
                    app.connect_robot();
                }
                return;
            }

            app.ui.mode.route = Route::from(last);
            // 进 Settings 时自动拉设备列表, picker 第一次打开就有数据
            if last == MenuItem::Settings {
                app.refresh_device_lists();
            }
            None
        }
        _ => None,
    };
    if let Some(e) = event {
        handle_event(app, e);
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
        let event = match code {
            KeyCode::Up => Some(AppEvent::Menu(MenuEvent::Up)),
            KeyCode::Down => Some(AppEvent::Menu(MenuEvent::Down)),
            KeyCode::Enter => Some(AppEvent::Menu(MenuEvent::EnterServoMode)),
            _ => None,
        };
        if let Some(e) = event {
            handle_event(app, e);
        }
        return;
    }
    // Active
    let event = match code {
        KeyCode::Enter => Some(AppEvent::Device(DeviceEvent::Exit)),
        KeyCode::Up => Some(AppEvent::Device(DeviceEvent::Prev)),
        KeyCode::Down => Some(AppEvent::Device(DeviceEvent::Next)),
        KeyCode::Left => Some(AppEvent::Device(DeviceEvent::Decrease)),
        KeyCode::Right => Some(AppEvent::Device(DeviceEvent::Increase)),
        KeyCode::Char('s') | KeyCode::Char('S') => Some(AppEvent::Device(DeviceEvent::Screenshot)),
        // F/f = 人脸追踪开关, 暂无对应 DeviceEvent, 直接调用 app 方法
        KeyCode::Char('f') | KeyCode::Char('F') => {
            app.toggle_face_tracking();
            None
        }
        _ => None,
    };
    if let Some(e) = event {
        handle_event(app, e);
    }
}

/// Settings 页面
///
/// 列表模式: Up/Down 选, Enter 进入编辑(对 Wifi 两项) 或设备选择器
/// (对麦克风/扬声器行), 'r' 刷新设备列表.
/// picker 模式 (selecting.is_some): 把按键翻译为 Picker*, 这里不发,
/// 由 handle_overlay 接管 DevicePicker 分支 (双路保险, 因为 overlay 已
/// 优先).
fn handle_settings(app: &mut App, code: KeyCode) {
    // picker 中 overlay 已优先处理, 这里理论上不会被命中,
    // 保留一份 translate 作防御
    if matches!(
        &app.ui.mode.route,
        Route::Settings {
            selecting: Some(_),
            ..
        }
    ) && app.ui.mode.overlay.is_none()
    {
        let event = match code {
            KeyCode::Up => Some(AppEvent::Settings(SettingsEvent::PickerUp)),
            KeyCode::Down => Some(AppEvent::Settings(SettingsEvent::PickerDown)),
            KeyCode::Enter => Some(AppEvent::Settings(SettingsEvent::PickerConfirm)),
            KeyCode::Esc => Some(AppEvent::Settings(SettingsEvent::PickerCancel)),
            KeyCode::Char('r') | KeyCode::Char('R') => {
                Some(AppEvent::Settings(SettingsEvent::PickerRefresh))
            }
            _ => None,
        };
        if let Some(e) = event {
            handle_event(app, e);
        }
        return;
    }
    let event = match code {
        KeyCode::Up => Some(AppEvent::Settings(SettingsEvent::Up)),
        KeyCode::Down => Some(AppEvent::Settings(SettingsEvent::Down)),
        KeyCode::Enter => {
            // 麦克风 / 扬声器行 (2/3) 走 picker; 其它 (0/1 Wifi) 走文本编辑
            let selected = if let Route::Settings { selected, .. } = &app.ui.mode.route {
                *selected
            } else {
                0
            };
            let kind = match selected {
                SETTINGS_IDX_SPEECH => Some(SelectingKind::Input),
                SETTINGS_IDX_OUTPUT => Some(SelectingKind::Output),
                _ => None,
            };
            if let Some(k) = kind {
                Some(AppEvent::Settings(SettingsEvent::EnterPicker(k)))
            } else {
                Some(AppEvent::Settings(SettingsEvent::Enter))
            }
        }
        KeyCode::Char('r') | KeyCode::Char('R') => {
            Some(AppEvent::Settings(SettingsEvent::RefreshList))
        }
        _ => None,
    };
    if let Some(e) = event {
        handle_event(app, e);
    }
}

fn handle_llm_test_route(app: &mut App, code: KeyCode) {
    handle_llm_test(app, code);
}

fn handle_tts_test_route(app: &mut App, code: KeyCode) {
    handle_tts_test(app, code);
}

fn handle_about(_app: &mut App, _code: KeyCode) {}

/// 把按键翻译为 `EditField` 上的具体操作
///
/// 由 `handle_overlay` 的 `Overlay::EditField` 分支调用; **只**关心
/// 怎么改 buffer/cursor, 不负责 commit/cancel (那两个走 Enter/Esc
/// 在 handle_overlay 单独处理).
///
/// ## 设计理由
///
/// 把按键语义下沉到 helper 而不是膨胀 `handle_overlay` 的 match arm,
/// 加新按键 (例如 Ctrl+A 全选 / Ctrl+K 删到末尾) 时只要往这里加分支,
/// 不动分发 match.
///
/// ## 字符 vs 控制键
///
/// crossterm 把 `Tab` 编为独立 `KeyCode::Tab`, **不会**走 `Char('\t')`
/// 分支; `Enter` 走自己的 `KeyCode::Enter` 也不到这. 所以 `Char(c)`
/// 分支可以安全接受任何 UTF-8 char, 不需要过滤控制键. 此假设若被
/// crossterm 升级打破, 需重新审视 (filter `c.is_control()`).
///
/// ## 未识别按键
///
/// 不在表内的 `KeyCode` (`F1`/`PageUp`/...) 原样穿透, 不动 buffer
/// 不动 cursor, 跟改前的 `_ => Some(Overlay::EditField(field))` 语义一致.
fn apply_edit_key(f: &mut EditField, code: KeyCode) {
    match code {
        KeyCode::Left => f.move_cursor_left(1),
        KeyCode::Right => f.move_cursor_right(1),
        KeyCode::Home => f.move_cursor_to_start(),
        KeyCode::End => f.move_cursor_to_end(),
        KeyCode::Backspace => {
            let _ = f.delete_back();
        }
        KeyCode::Delete => {
            let _ = f.delete_forward();
        }
        KeyCode::Char(c) => f.insert_char(c),
        _ => {} // 其它按键 (Tab / F1-12 / PageUp 等) 不动 buffer
    }
}

/// 模态层: overlay 优先消费按键
fn handle_overlay(app: &mut App, code: KeyCode) {
    let overlay = app.ui.mode.overlay.take();
    match overlay {
        Some(Overlay::EditField(field)) => {
            let new_overlay = match code {
                KeyCode::Esc => {
                    app.cancel_settings_edit();
                    None
                }
                KeyCode::Enter => {
                    // 把当前 overlay 字段同步回 Route.editing, 再让 commit 取走
                    if let Route::Settings { editing, .. } = &mut app.ui.mode.route {
                        *editing = Some(field);
                    }
                    app.commit_settings_edit();
                    None
                }
                // 编辑按键 (Left/Right/Home/End/Delete/Backspace/Char) 全部走
                // [`apply_edit_key`], 该辅助集中翻译为 EditField 操作. 任何未识别
                // 的 KeyCode (例如 Tab, F1) 原样穿透, 不改 buffer 也不改 cursor —
                // 跟现状兼容, 避免未来再加新按键时让 match 越来越胖.
                code => {
                    let mut f = field;
                    apply_edit_key(&mut f, code);
                    // buffer / cursor 改变后回写 Route.editing, 让 viewmodel 下一帧
                    // 拿到新值; 同时把 f 放回 overlay 供后续按键继续.
                    if let Route::Settings { editing, .. } = &mut app.ui.mode.route {
                        *editing = Some(f.clone());
                    }
                    Some(Overlay::EditField(f))
                }
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
        Some(Overlay::DevicePicker { selecting, devices }) => {
            // picker 方法内部通过 `&mut self.ui.mode.overlay` 改 cursor / devices,
            // 所以必须先把 overlay 放回去, 否则取走的瞬间所有方法都看不到它.
            // (上游 take() 之后, 这里 overlay 字段已被 moved 出来, 用它重建)
            app.ui.mode.overlay = Some(Overlay::DevicePicker { selecting, devices });
            match code {
                KeyCode::Esc => {
                    app.cancel_device_picker();
                }
                KeyCode::Enter => {
                    app.confirm_device_picker();
                }
                KeyCode::Char('r') | KeyCode::Char('R') => {
                    app.refresh_device_picker();
                }
                KeyCode::Up => {
                    app.picker_up();
                }
                KeyCode::Down => {
                    app.picker_down();
                }
                _ => {}
            }
        }
        Some(Overlay::DeviceSwitchFailure {
            kind,
            old_device_name,
            detail,
        }) => {
            // 同上, dismiss_device_failure 要看 overlay, 先放回去
            app.ui.mode.overlay = Some(Overlay::DeviceSwitchFailure {
                kind,
                old_device_name,
                detail,
            });
            if code == KeyCode::Esc {
                app.dismiss_device_failure();
            }
        }
        None => unreachable!("guarded by caller"),
    }
}
