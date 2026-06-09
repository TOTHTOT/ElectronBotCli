//! 设置事件

use crate::app::App;

/// 设置事件
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsEvent {
    Exit,
    Up,
    Down,
    EnterEdit,
    Save,
}

/// 处理设置事件(由 input::handle_by_mode 路由, 已是 Settings 模式)
pub fn handle(app: &mut App, event: SettingsEvent) {
    match event {
        SettingsEvent::Exit => {
            // 退出 Settings 回 Nav
        }
        SettingsEvent::Up => app.settings_prev(),
        SettingsEvent::Down => app.settings_next(),
        SettingsEvent::EnterEdit => {
            app.begin_settings_edit();
        }
        SettingsEvent::Save => {
            log::info!("Saving settings");
        }
    }
}
