//! 设置事件
//!
//! `SettingsEvent` 覆盖 Settings 列表页的按键语义(Up/Down/Enter/R)。
//! - `EditField` overlay 是另一种输入模式, 由 `handle_overlay` 直接处理
//!   (逐字符输入/Backspace/Enter 提交), 不走 `SettingsEvent`.
//! - `DevicePicker` overlay 同样由 `handle_overlay` 直接处理
//!   (Up/Down/Enter/Esc/'r'), 这里只发"进入 picker"事件.

use crate::app::route::SelectingKind;
use crate::app::App;
use crate::app::Overlay;

/// 设置事件
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsEvent {
    /// 上一个设置项
    Up,
    /// 下一个设置项
    Down,
    /// 进入当前项的编辑 (仅对 `Wifi/wifi_password/speech_name` 生效, 扬声器走 `EnterPicker`)
    Enter,
    /// 进入当前项的设备选择器(麦克风/扬声器行)
    EnterPicker(SelectingKind),
    /// 列表模式按 R — 重新拉取设备列表
    RefreshList,
    /// picker 内 ↑ (`SettingsEvent` 镜像, 也可由 `handle_overlay` 直接处理)
    PickerUp,
    /// picker 内 ↓
    PickerDown,
    /// picker 内 Enter — 提交选择
    PickerConfirm,
    /// picker 内 Esc — 取消
    PickerCancel,
0    /// 音量条 ←→ 调节 (仅音量行选中时生效, 其它行 no-op)
    VolumeAdjust(i8),
    /// picker 内 R — 重新拉列表, 保留 cursor 框架
    PickerRefresh,
}

/// 处理设置事件 (列表模式 / picker 模式)
pub fn handle(app: &mut App, event: SettingsEvent) {
    match event {
        SettingsEvent::Up => app.settings_prev(),
        SettingsEvent::Down => app.settings_next(),
        SettingsEvent::Enter => begin_edit(app),
        SettingsEvent::EnterPicker(kind) => {
            app.enter_device_picker(kind);
        }
        SettingsEvent::RefreshList => app.refresh_device_lists_in_settings(),
        SettingsEvent::VolumeAdjust(delta) => app.adjust_volume(delta),
        SettingsEvent::PickerUp => app.picker_up(),
        SettingsEvent::PickerDown => app.picker_down(),
        SettingsEvent::PickerConfirm => app.confirm_device_picker(),
        SettingsEvent::PickerCancel => app.cancel_device_picker(),
        SettingsEvent::PickerRefresh => app.refresh_device_picker(),
    }
}

/// 进入设置项编辑: 写入 `Route::Settings.editing` 并弹出 overlay
fn begin_edit(app: &mut App) {
    app.begin_settings_edit();
    if let crate::app::Route::Settings { editing, .. } = &app.ui.mode.route {
        if let Some(field) = editing.clone() {
            app.ui.mode.overlay = Some(Overlay::EditField(field));
        }
    }
}
