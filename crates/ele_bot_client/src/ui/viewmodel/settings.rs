use crate::app::overlay::{DeviceKind, Overlay};
use crate::app::route::{Route, SelectingKind};
use crate::app::App;
use std::time::Instant;

#[allow(dead_code)]
pub struct SettingsViewModel {
    pub settings_items: Vec<SettingItem>,
    pub selected_index: usize,
    pub in_edit_mode: bool,
    pub edit_buffer: String,
    pub picker: Option<PickerVm>,
    pub failure_overlay: Option<FailureVm>,
}

#[allow(dead_code)]
pub struct SettingItem {
    pub label: &'static str,
    pub value: String,
}

/// picker 视图模型 — Overlay::DevicePicker 的纯数据投影
#[allow(dead_code)]
pub struct PickerVm {
    pub kind: SelectingKind,
    pub loading: bool,
    pub rows: Vec<PickerRow>,
    pub cursor: usize,
}

#[allow(dead_code)]
pub struct PickerRow {
    pub label: String,
    /// dim gray 提示后缀 (e.g. "WASAPI 麦克风阵列 (2ch / 48000Hz)")
    /// 与 `highlighted_name` 一起拼成完整文本
    pub highlighted_name: String,
    pub dim_suffix: String,
}

/// failure transient 视图模型
#[allow(dead_code)]
pub struct FailureVm {
    pub kind: DeviceKind,
    pub old_device_name: String,
    pub detail: String,
    /// 弹出时刻 (用于 5 秒自动关)
    pub opened_at: Instant,
}

impl SettingsViewModel {
    pub fn from_app(app: &App) -> Self {
        let mut items = vec![
            SettingItem {
                label: "Wifi名称",
                value: app.config.wifi_ssid.clone(),
            },
            SettingItem {
                label: "Wifi密码",
                value: app.config.wifi_password.clone(),
            },
        ];

        // 麦克风 / 扬声器: 显示当前选择的 display (查设备缓存, 找不到回退原值)
        items.push(SettingItem {
            label: "麦克风",
            value: display_for(&app.devices.inputs, &app.config.speech_name),
        });
        items.push(SettingItem {
            label: "扬声器",
            value: display_for(&app.devices.outputs, &app.config.output_device),
        });

        // 从 Route::Settings 取 selected + editing
        let (selected_index, in_edit_mode, edit_buffer, selecting) = match &app.ui.mode.route {
            Route::Settings {
                editing: Some(f), ..
            } => (f.index, true, f.buffer.clone(), None),
            Route::Settings {
                selected,
                selecting: Some(s),
                ..
            } => (*selected, false, String::new(), Some(s.clone())),
            Route::Settings { selected, .. } => (*selected, false, String::new(), None),
            _ => (0, false, String::new(), None),
        };

        let picker = match &app.ui.mode.overlay {
            Some(Overlay::DevicePicker { selecting, devices }) => {
                let mut rows: Vec<PickerRow> = Vec::with_capacity(devices.len() + 1);
                // idx 0: <系统默认>
                rows.push(PickerRow {
                    label: "系统默认".to_string(),
                    highlighted_name: String::new(),
                    dim_suffix: String::new(),
                });
                for d in devices {
                    let (name, suffix) = split_driver_and_suffix(d);
                    rows.push(PickerRow {
                        label: d.display.clone(),
                        highlighted_name: name,
                        dim_suffix: suffix,
                    });
                }
                Some(PickerVm {
                    kind: selecting.kind,
                    loading: selecting.loading,
                    cursor: selecting.cursor,
                    rows,
                })
            }
            _ => None,
        };

        let failure_overlay = match &app.ui.mode.overlay {
            Some(Overlay::DeviceSwitchFailure {
                kind,
                old_device_name,
                detail,
            }) => Some(FailureVm {
                kind: *kind,
                old_device_name: old_device_name.clone(),
                detail: detail.clone(),
                opened_at: Instant::now(),
            }),
            _ => None,
        };

        let _ = selecting; // suppress unused if pattern destructured

        Self {
            settings_items: items,
            selected_index,
            in_edit_mode,
            edit_buffer,
            picker,
            failure_overlay,
        }
    }
}

/// 把选中的 device 名字映射成 "display or <系统默认>"
fn display_for(devices: &[ele_bot_proto::DeviceInfoDto], name: &str) -> String {
    if name.is_empty() {
        "<系统默认>".to_string()
    } else {
        devices
            .iter()
            .find(|d| d.name == name)
            .map(|d| d.display.clone())
            .unwrap_or_else(|| name.to_string())
    }
}

/// 把 DTO 的 display 拆成 "name" 和 "suffix" 两段, 给 UI 高亮 / dim 区分.
///
/// `display` 形如 "WASAPI 麦克风阵列 (2ch, 48000Hz)". 我们以第一个 '('
/// 为切点, 前半截继续按首个空格拆 driver / name; 后半截整体 dim.
fn split_driver_and_suffix(d: &ele_bot_proto::DeviceInfoDto) -> (String, String) {
    let display = &d.display;
    if let Some(open) = display.find('(') {
        let head = &display[..open];
        let tail = &display[open..];
        let trimmed_tail = tail.trim_end().to_string();
        let name = if d.driver.is_some() {
            // driver 已被 caller 分离, 这里直接把整段当作 "name (无 driver 时也成立)"
            head.trim().to_string()
        } else {
            head.trim().to_string()
        };
        (name, trimmed_tail)
    } else {
        (display.clone(), String::new())
    }
}
