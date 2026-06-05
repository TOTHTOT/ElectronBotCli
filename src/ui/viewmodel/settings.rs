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
                value: app.config.microphone_name.clone(),
            },
        ];

        Self {
            settings_items,
            selected_index: app.ui.settings_selected,
            in_edit_mode: app.ui.in_edit_settings_mode,
            edit_buffer: app.ui.edit_buffer.clone(),
        }
    }
}
