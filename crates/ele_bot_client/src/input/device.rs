//! 设备控制事件

use crate::app::App;

/// 设备控制事件
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceEvent {
    /// 退出 Active 模式, 回到 Idle (Enter on Active)
    Exit,
    /// 上一个舵机
    Prev,
    /// 下一个舵机
    Next,
    /// 当前舵机角度减
    Decrease,
    /// 当前舵机角度增
    Increase,
    /// 截图
    Screenshot,
}

/// 处理设备控制事件 (Active 模式)
pub fn handle(app: &mut App, event: DeviceEvent) {
    match event {
        DeviceEvent::Exit => app.enter_device_control_idle(),
        DeviceEvent::Next => app.next_servo(),
        DeviceEvent::Prev => app.prev_servo(),
        DeviceEvent::Increase => app.increase_selected(),
        DeviceEvent::Decrease => app.decrease_selected(),
        DeviceEvent::Screenshot => app.take_screenshot(),
    }
}
