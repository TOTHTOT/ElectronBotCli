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

/// 处理菜单事件
pub fn handle(app: &mut App, event: MenuEvent) {
    // 如果在舵机模式或设置模式中，不处理菜单事件
    if app.in_servo_mode || app.in_settings {
        return;
    }

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
            if matches!(app.selected_menu, crate::app::MenuItem::DeviceControl) {
                app.in_servo_mode = true;
                // 进入设备控制页面时，焦点切换到右侧
                app.left_focused = false;
            }
        }
        MenuEvent::EnterSettingMode => {
            if app.selected_menu == crate::app::MenuItem::Settings {
                app.in_settings = true;
                // 进入设置页面时，焦点切换到右侧
                app.left_focused = false;
            }
        }
    }
}
