//! 设备控制事件

use crate::app::App;

/// 设备控制事件
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum DeviceEvent {
    Exit,
    Next,
    Prev,
    Increase,
    Decrease,
    Screenshot,
}

/// 处理设备控制事件(由 input::handle_by_mode 路由, 已是 DeviceControl 模式)
pub fn handle(app: &mut App, event: DeviceEvent) {
    match event {
        DeviceEvent::Exit => {
            // 退到 Idle
        }
        DeviceEvent::Next => app.next_servo(),
        DeviceEvent::Prev => app.prev_servo(),
        DeviceEvent::Increase => app.increase_selected(),
        DeviceEvent::Decrease => app.decrease_selected(),
        DeviceEvent::Screenshot => app.take_screenshot(),
    }
}
