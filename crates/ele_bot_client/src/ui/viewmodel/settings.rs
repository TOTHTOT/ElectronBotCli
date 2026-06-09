use crate::app::route::Route;
use crate::app::App;

#[allow(dead_code)]
pub struct SettingsViewModel {
    pub settings_items: Vec<SettingItem>,
    pub selected_index: usize,
    pub in_edit_mode: bool,
    pub edit_buffer: String,
}

#[allow(dead_code)]
pub struct SettingItem {
    pub label: &'static str,
    pub value: String,
}

impl SettingsViewModel {
    pub fn from_app(app: &App) -> Self {
        let settings_items = vec![
            SettingItem {
                label: "Wifi名称",
                value: app.config.wifi_ssid.clone(),
            },
            SettingItem {
                label: "Wifi密码",
                value: app.config.wifi_password.clone(),
            },
            SettingItem {
                label: "麦克风名称",
                value: app.config.speech_name.clone(),
            },
        ];

        // 从 Route::Settings 取 selected + editing
        let (selected_index, in_edit_mode, edit_buffer) = match &app.ui.mode.route {
            Route::Settings {
                selected: _,
                editing: Some(f),
            } => (f.index, true, f.buffer.clone()),
            Route::Settings { selected, .. } => (*selected, false, String::new()),
            _ => (0, false, String::new()),
        };

        Self {
            settings_items,
            selected_index,
            in_edit_mode,
            edit_buffer,
        }
    }
}
