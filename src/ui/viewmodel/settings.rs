use crate::app::App;
use crate::media::voice::DeviceInfo;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingItemKind {
    Text,
    DeviceInput,
    DeviceOutput,
}

#[allow(dead_code)]
pub struct SettingItem {
    pub label: &'static str,
    pub value: String,
    pub kind: SettingItemKind,
}

#[allow(dead_code)]
pub struct SettingsViewModel {
    pub settings_items: Vec<SettingItem>,
    pub selected_index: usize,
    pub in_edit_mode: bool,
    pub edit_buffer: String,
    pub in_device_selection_mode: bool,
    pub device_selection_index: usize,
    pub input_devices: Vec<DeviceInfo>,
    pub output_devices: Vec<DeviceInfo>,
}

#[allow(dead_code)]
impl SettingsViewModel {
    pub fn from_app(app: &App) -> Self {
        let settings_items = vec![
            SettingItem {
                label: "Wifi名称",
                value: app.config.wifi_ssid.clone(),
                kind: SettingItemKind::Text,
            },
            SettingItem {
                label: "Wifi密码",
                value: app.config.wifi_password.clone(),
                kind: SettingItemKind::Text,
            },
            SettingItem {
                label: "输入设备",
                value: app.config.speech_name.clone(),
                kind: SettingItemKind::DeviceInput,
            },
            SettingItem {
                label: "输出设备",
                value: if app.config.output_device.is_empty() {
                    "(默认)".to_string()
                } else {
                    app.config.output_device.clone()
                },
                kind: SettingItemKind::DeviceOutput,
            },
        ];

        Self {
            settings_items,
            selected_index: app.ui.settings_selected,
            in_edit_mode: app.ui.in_edit_settings_mode,
            edit_buffer: app.ui.edit_buffer.clone(),
            in_device_selection_mode: app.ui.in_device_selection_mode,
            device_selection_index: app.ui.device_selection_index,
            input_devices: app.input_devices.clone(),
            output_devices: app.output_devices.clone(),
        }
    }
}
