/// 常量定义
pub const SERVO_COUNT: usize = 6;
pub const SERVO_MIN: i16 = -180;
pub const SERVO_MAX: i16 = 180;

// LCD 帧尺寸
pub const FRAME_WIDTH: usize = 240;
pub const FRAME_HEIGHT: usize = 240;
pub const FRAME_DEPTH: usize = 3; // RGB888
pub const FRAME_SIZE: usize = FRAME_WIDTH * FRAME_HEIGHT * FRAME_DEPTH; // 172800 bytes
