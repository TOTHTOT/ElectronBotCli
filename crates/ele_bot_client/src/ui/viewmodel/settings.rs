use crate::app::overlay::{DeviceKind, Overlay, PickerEntry};
use crate::app::route::{Route, SelectingKind};
use crate::app::App;
use std::time::Instant;

#[allow(dead_code)]
pub struct SettingsViewModel {
    pub settings_items: Vec<SettingItem>,
    pub selected_index: usize,
    pub in_edit_mode: bool,
    pub edit_buffer: String,
    /// 编辑态 caret 的字符索引, 由渲染层 (`pages/settings.rs::render_setting_item`)
    /// 用于把 buffer 拆成 `before + caret + after` 三段渲染. 非编辑态值无意义,
    /// 渲染层只用 `in_edit_mode` 当 gate.
    pub edit_cursor: usize,
    pub picker: Option<PickerVm>,
    pub failure_overlay: Option<FailureVm>,
}

#[allow(dead_code)]
pub struct SettingItem {
    pub label: &'static str,
    pub value: String,
}

/// picker 视图模型 — `Overlay::DevicePicker` 的纯数据投影
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
        // 摄像头: 显示当前 index, 查 cameras 缓存拿 display 替换.
        items.push(SettingItem {
            label: "摄像头",
            value: display_for_camera(&app.devices.cameras, &app.config.camera_index),
        });

        // 从 Route::Settings 取 selected + editing + cursor
        let (selected_index, in_edit_mode, edit_buffer, edit_cursor, selecting) =
            match &app.ui.mode.route {
                Route::Settings {
                    editing: Some(f), ..
                } => (f.index, true, f.buffer.clone(), f.cursor, None),
                Route::Settings {
                    selected,
                    selecting: Some(s),
                    ..
                } => (*selected, false, String::new(), 0, Some(s.clone())),
                Route::Settings { selected, .. } => (*selected, false, String::new(), 0, None),
                _ => (0, false, String::new(), 0, None),
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
                // Audio 走 split_driver_and_suffix 把 name / 后缀拆开,
                // 让 driver 段亮, 通道数段 dim. Camera 没有 driver 后缀,
                // 整行都亮.
                for entry in devices {
                    let (label, suffix) = match entry {
                        PickerEntry::Audio(d) => {
                            let (name, suf) = split_driver_and_suffix(d);
                            let full = if suf.is_empty() {
                                name
                            } else if name.is_empty() {
                                suf
                            } else {
                                format!("{name} {suf}")
                            };
                            (full, String::new())
                        }
                        PickerEntry::Camera(d) => (d.display.clone(), String::new()),
                    };
                    rows.push(PickerRow {
                        label,
                        highlighted_name: String::new(),
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
            edit_cursor,
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
            .map_or_else(|| name.to_string(), |d| d.display.clone())
    }
}

/// 把当前 `camera_index` 字符串映射成 "display or <系统默认>"
///
/// 摄像头没有 OS 标准命名差异 (不像 cpal 设备), 所以按 `id` 严格匹配.
/// `id` 为空时显示 `<系统默认>`. 找不到对应设备 (还没收到 `Cameras` 事件)
/// 时回退到原 id 字符串.
fn display_for_camera(devices: &[ele_bot_proto::CameraInfoDto], id: &str) -> String {
    if id.is_empty() {
        return "<系统默认>".to_string();
    }
    devices
        .iter()
        .find(|d| d.id == id)
        .map_or_else(|| id.to_string(), |d| d.display.clone())
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
