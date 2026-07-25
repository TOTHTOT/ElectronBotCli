//! `AppMode` - 路由 + 模态的组合
//!
//! 把原来散落在 `UiState` 上的 5 个 mode bool + `left_focused` + popup
//! 合并成二层结构, 由编译器保证互斥。

use super::menu::MenuItem;
use super::overlay::Overlay;
use super::route::{DeviceControlMode, Route};

/// 应用当前模式: 当前页 + 可能的模态
#[derive(Debug, Clone)]
pub struct AppMode {
    pub route: Route,
    pub overlay: Option<Overlay>,
}

impl AppMode {
    #[must_use] 
    pub fn new() -> Self {
        Self {
            route: Route::Nav {
                last_entered: MenuItem::DeviceStatus,
            },
            overlay: None,
        }
    }

    /// 侧边栏获得视觉焦点 = 右面板不接收输入。
    /// 推导规则(取代旧 `left_focused: bool`):
    /// - `Nav` / `About` / `DeviceControl::Idle`: 侧边栏高亮
    /// - 其它 Route 或有 overlay: 右面板高亮
    #[must_use] 
    pub fn sidebar_focused(&self) -> bool {
        if self.overlay.is_some() {
            return false;
        }
        match &self.route {
            Route::Nav { .. } | Route::About => true,
            Route::DeviceControl { mode } => *mode == DeviceControlMode::Idle,
            _ => false,
        }
    }
}

impl Default for AppMode {
    fn default() -> Self {
        Self::new()
    }
}
