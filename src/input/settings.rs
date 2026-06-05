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

/// 处理设置事件
pub fn handle(app: &mut App, event: SettingsEvent) {
    if !app.ui.in_settings {
        return;
    }

    if app.ui.in_edit_settings_mode {
        return;
    }

    match event {
        SettingsEvent::Exit => {
            app.ui.in_settings = false;
        }
        SettingsEvent::Up => app.settings_prev(),
        SettingsEvent::Down => app.settings_next(),
        SettingsEvent::EnterEdit => {
            app.ui.in_edit_settings_mode = true;
            app.ui.edit_buffer = match app.ui.settings_selected {
                0 => app.config.wifi_ssid.clone(),
                1 => app.config.wifi_password.clone(),
                2 => app.config.microphone_name.clone(),
                _ => String::new(),
            };
        }
        SettingsEvent::Save => {
            log::info!("Saving settings");
        }
    }
}
