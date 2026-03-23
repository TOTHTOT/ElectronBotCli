use crate::app::App;
use crate::robot::{ServoState, SERVO_COUNT};

pub struct DeviceControlViewModel {
    pub joint_values: Vec<i16>,
    pub selected_servo: usize,
    pub is_servo_mode: bool,
    pub servo_names: Vec<&'static str>,
    pub servo_ranges: Vec<String>,
}

impl DeviceControlViewModel {
    pub fn from_app(app: &App) -> Self {
        let values = app.joint.values();
        let joint_values: Vec<i16> = values.to_vec();
        let selected_servo = app.joint.selected();

        let mut servo_names = Vec::with_capacity(SERVO_COUNT);
        let mut servo_ranges = Vec::with_capacity(SERVO_COUNT);
        for i in 0..SERVO_COUNT {
            servo_names.push(ServoState::name(i));
            servo_ranges.push(ServoState::range_str(i));
        }

        Self {
            joint_values,
            selected_servo,
            is_servo_mode: app.in_servo_mode,
            servo_names,
            servo_ranges,
        }
    }
}
