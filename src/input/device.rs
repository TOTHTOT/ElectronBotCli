//! 设备控制事件

use crate::app::App;

/// 设备控制事件
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceEvent {
    Exit,
    Next,
    Prev,
    Increase,
    Decrease,
    Screenshot,
}

/// 处理设备控制事件
pub fn handle(app: &mut App, event: DeviceEvent) {
    if !app.in_servo_mode {
        return;
    }

    match event {
        DeviceEvent::Exit => {
            app.in_servo_mode = false;
        }
        DeviceEvent::Next => app.joint.next_servo(),
        DeviceEvent::Prev => app.joint.prev_servo(),
        DeviceEvent::Increase => app.joint.increase(),
        DeviceEvent::Decrease => app.joint.decrease(),
        DeviceEvent::Screenshot => {
            if let Err(e) = app.take_screenshot() {
                log::error!("Screenshot failed: {}", e);
            }
        }
    }
}
