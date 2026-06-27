//! 设置事件
//!
//! SettingsEvent 只覆盖 Settings 列表页的按键语义(Up/Down/Enter).
//! EditField overlay 是另一种输入模式, 由 handle_overlay 直接处理
//! (逐字符输入/Backspace/Enter 提交), 不走 SettingsEvent.

use crate::app::App;
use crate::app::Overlay;

/// 设置事件
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsEvent {
    /// 上一个设置项
    Up,
    /// 下一个设置项
    Down,
    /// 进入当前项的编辑 (begin + 弹 overlay)
    Enter,
}

/// 处理设置事件 (列表模式)
pub fn handle(app: &mut App, event: SettingsEvent) {
    match event {
        SettingsEvent::Up => app.settings_prev(),
        SettingsEvent::Down => app.settings_next(),
        SettingsEvent::Enter => begin_edit(app),
    }
}

/// 进入设置项编辑: 写入 Route::Settings.editing 并弹出 overlay
fn begin_edit(app: &mut App) {
    app.begin_settings_edit();
    if let crate::app::Route::Settings { editing, .. } = &app.ui.mode.route {
        if let Some(field) = editing.clone() {
            app.ui.mode.overlay = Some(Overlay::EditField(field));
        }
    }
}
