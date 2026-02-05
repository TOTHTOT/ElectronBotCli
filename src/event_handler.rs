use crate::app::App;

pub enum AppEvent {
    Quit,
    MenuUp,
    MenuDown,
    ConnectDevice,
    EnterServoMode,
    ExitServoMode,
    ServoNext,
    ServoPrev,
    ServoIncrease,
    ServoDecrease,
    ServoIncreaseBig,
    ServoDecreaseBig,
    None,
}

pub fn handle_event(app: &mut App, event: AppEvent) {
    // 如果在舵机模式，优先处理舵机事件
    if app.in_servo_mode {
        match event {
            AppEvent::Quit => {
                app.in_servo_mode = false;
            }
            AppEvent::ExitServoMode => {
                app.in_servo_mode = false;
            }
            AppEvent::ServoNext => app.servo_state.next_servo(),
            AppEvent::ServoPrev => app.servo_state.prev_servo(),
            AppEvent::ServoIncrease => app.servo_state.increase(),
            AppEvent::ServoDecrease => app.servo_state.decrease(),
            AppEvent::ServoIncreaseBig => app.servo_state.increase_big(),
            AppEvent::ServoDecreaseBig => app.servo_state.decrease_big(),
            _ => {}
        }
        return;
    }

    // 普通菜单模式
    match event {
        AppEvent::Quit => app.quit(),
        AppEvent::MenuUp => app.prev_menu(),
        AppEvent::MenuDown => app.next_menu(),
        AppEvent::ConnectDevice => {
            // 如果已连接则断开，否则连接
            if app.device.is_connected() {
                app.disconnect_device();
            } else {
                // 尝试连接第一个可用设备
                if let Some(port) = crate::device::CdcDevice::list_ports().first() {
                    app.connect_device(port);
                }
            }
        }
        AppEvent::EnterServoMode => {
            if matches!(app.selected_menu, crate::app::MenuItem::DeviceControl) {
                app.in_servo_mode = true;
            }
        }
        _ => {}
    }
}
