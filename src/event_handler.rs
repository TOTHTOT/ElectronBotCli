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
            // 如果已连接则断开
            if app.device.is_connected() {
                app.disconnect_device();
            } else {
                // 如果端口选择弹窗已经打开，使用选中的端口
                if app.port_select_popup.is_visible() {
                    if let Some(port) = app.port_select_popup.selected_port() {
                        let port_name = port.to_string();
                        app.connect_device(&port_name);
                    }
                } else {
                    // 否则打开端口选择弹窗
                    app.port_select_popup.show();
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
