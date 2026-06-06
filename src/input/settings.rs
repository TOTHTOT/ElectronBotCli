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

    // 设备选择模式由专门的 handler 处理
    if app.ui.in_device_selection_mode {
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
        SettingsEvent::EnterEdit => match app.ui.settings_selected {
            // 文本编辑项：进入文本编辑模式
            0 | 1 => {
                app.ui.in_edit_settings_mode = true;
                app.ui.edit_buffer = match app.ui.settings_selected {
                    0 => app.config.wifi_ssid.clone(),
                    1 => app.config.wifi_password.clone(),
                    _ => String::new(),
                };
            }
            // 设备选择项：进入设备选择模式
            2 | 3 => {
                let devices: &[String] = match app.ui.settings_selected {
                    2 => &app.input_devices,
                    3 => &app.output_devices,
                    _ => &[],
                };
                app.ui.in_device_selection_mode = true;
                // 初始化为当前 config 设备名在列表中的位置, 找不到则 0
                let current_name = match app.ui.settings_selected {
                    2 => app.config.speech_name.clone(),
                    3 => app.config.output_device.clone(),
                    _ => String::new(),
                };
                let pos = devices
                    .iter()
                    .position(|d| d == &current_name)
                    .unwrap_or(0);
                app.ui.device_selection_index = pos.min(devices.len().saturating_sub(1));
            }
            _ => {}
        },
        SettingsEvent::Save => {
            log::info!("Saving settings");
        }
    }
}
