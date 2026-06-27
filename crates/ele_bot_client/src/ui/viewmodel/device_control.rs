use crate::app::App;
use ele_bot_proto::SERVO_COUNT;

/// 舵机名称(与服务端 robot::joint::SERVOS 保持一致)
const SERVO_NAMES: [&str; SERVO_COUNT] = ["头部", "左肩", "左臂", "右肩", "右臂", "身体"];

/// 舵机角度范围
const SERVO_RANGES: [(i16, i16); SERVO_COUNT] = [
    (-15, 15),
    (-30, 30),
    (-180, 180),
    (-30, 30),
    (-180, 180),
    (-90, 90),
];

pub struct DeviceControlViewModel {
    pub joint_values: Vec<i16>,
    pub selected_servo: usize,
    pub is_servo_mode: bool,
    pub servo_names: Vec<&'static str>,
    pub servo_ranges: Vec<String>,
}

impl DeviceControlViewModel {
    pub fn from_app(app: &App) -> Self {
        let joint_values: Vec<i16> = app.joint_values().to_vec();
        let selected_servo = app.joint_selected();

        let servo_names: Vec<&'static str> = SERVO_NAMES.to_vec();
        let servo_ranges: Vec<String> = SERVO_RANGES
            .iter()
            .map(|(min, max)| format!("{}° ~ {}°", min, max))
            .collect();

        Self {
            joint_values,
            selected_servo,
            is_servo_mode: false, // 由 ui.in_settings 决定, 这里占位
            servo_names,
            servo_ranges,
        }
    }
}
