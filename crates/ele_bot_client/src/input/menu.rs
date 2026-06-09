//! 菜单事件

use crate::app::App;

/// 菜单事件
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuEvent {
    Up,
    Down,
    ConnectDevice,
    EnterServoMode,
    EnterSettingMode,
}

/// 处理菜单事件(由 input::handle_by_mode 路由, 已是 Nav 模式)
pub fn handle(app: &mut App, event: MenuEvent) {
    match event {
        MenuEvent::Up => app.prev_menu(),
        MenuEvent::Down => app.next_menu(),
        MenuEvent::ConnectDevice => {
            if app.is_connected() {
                app.stop_comm_thread();
            } else {
                app.connect_robot();
            }
        }
        MenuEvent::EnterServoMode => {
            // 兼容性入口, 实际由 handle_nav Enter 直接 Route::from 触发
            app.enter_device_control_active();
        }
        MenuEvent::EnterSettingMode => {
            // 同上
        }
    }
}
