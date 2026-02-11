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
    Screenshot,
    None,
}

pub fn handle_event(app: &mut App, event: AppEvent) {
    if app.in_servo_mode {
        match event {
            AppEvent::Quit | AppEvent::ExitServoMode => {
                app.in_servo_mode = false;
            }
            AppEvent::ServoNext => app.joint.next_servo(),
            AppEvent::ServoPrev => app.joint.prev_servo(),
            AppEvent::ServoIncrease => app.joint.increase(),
            AppEvent::ServoDecrease => app.joint.decrease(),
            AppEvent::Screenshot => {
                if let Err(e) = app.take_screenshot() {
                    log::error!("Screenshot failed: {e}");
                }
            }
            _ => {}
        }
        return;
    }

    match event {
        AppEvent::Quit => app.quit(),
        AppEvent::MenuUp => app.prev_menu(),
        AppEvent::MenuDown => app.next_menu(),
        AppEvent::ConnectDevice => {
            if app.is_connected() {
                app.stop_comm_thread();
            } else {
                app.connect_robot();
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
