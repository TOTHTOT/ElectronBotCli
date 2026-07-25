//! 模态层 - 叠加在 Route 之上的瞬时状态
//!
//! 当 `AppMode::overlay` 为 `Some` 时, 所有按键优先路由到 overlay。

use super::route::{EditField, SelectingKind};
use ele_bot_proto::DeviceInfoDto;
use ratatui::style::Color;

/// 设备切换失败时, 服务器/客户端之间保留的旧设备名 + 失败时间
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    Input,
    Output,
}

impl From<SelectingKind> for DeviceKind {
    fn from(k: SelectingKind) -> Self {
        match k {
            SelectingKind::Input => DeviceKind::Input,
            SelectingKind::Output => DeviceKind::Output,
        }
    }
}

/// 弹窗内容配置
#[derive(Debug, Clone)]
pub struct PopupConfig {
    pub title: String,
    pub content: String,
    pub width: u16,
    pub height: u16,
    pub border_color: Color,
    pub bg_color: Color,
    pub title_color: Color,
}

impl Default for PopupConfig {
    fn default() -> Self {
        Self {
            title: "弹窗".to_string(),
            content: String::new(),
            width: 40,
            height: 5,
            border_color: Color::Green,
            bg_color: Color::DarkGray,
            title_color: Color::Cyan,
        }
    }
}

impl PopupConfig {
    /// "正在连接设备..." 默认配置
    #[must_use]
    pub fn connecting() -> Self {
        Self {
            title: " 连接设备 ".to_string(),
            content: "正在连接设备...".to_string(),
            ..Self::default()
        }
    }

    /// "确认退出" 默认配置
    #[must_use]
    pub fn confirm_quit() -> Self {
        Self {
            title: " 确认退出 ".to_string(),
            content: "确定要退出程序吗?\n[Enter] 确认   [Esc] 取消".to_string(),
            border_color: Color::Yellow,
            title_color: Color::Yellow,
            ..Self::default()
        }
    }
}

/// 弹窗 Esc 键的行为
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupDismiss {
    /// 仅关闭弹窗
    Cancel,
    /// 关闭弹窗 + 调用 `stop_comm_thread(用于"连接中"可中断)`
    CancelConnect,
    /// Esc 关闭弹窗(取消), Enter 确认退出程序
    ConfirmQuit,
}

/// 模态层
#[derive(Debug, Clone)]
pub enum Overlay {
    /// 文本编辑模态(设置项编辑)
    EditField(EditField),
    /// 弹窗(连接中/确认等)
    Popup {
        config: PopupConfig,
        on_dismiss: PopupDismiss,
    },
    /// 设备选择器 — 与 `EditField` 平行的另一种模态
    DevicePicker {
        /// 嵌入式子状态镜像 (与 `Route::Settings::selecting` 同步)
        selecting: super::route::SelectingField,
        /// 当前持有的设备列表 (来自最近一次 `*Devices` 事件)
        devices: Vec<DeviceInfoDto>,
    },
    /// 设备切换失败的 transient 提示 — Esc 关, 5 秒后自动关
    DeviceSwitchFailure {
        kind: DeviceKind,
        /// 用户提交切换的目标设备名 (失败时的目标)
        old_device_name: String,
        /// 来自服务端 `ServerEvent::Error` 的失败明细
        detail: String,
    },
}
