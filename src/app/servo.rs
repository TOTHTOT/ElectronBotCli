use crate::app::constants::{SERVO_COUNT, SERVO_MAX, SERVO_MIN};
use crate::device::JointConfig;

/// 舵机状态
#[derive(Clone)]
pub struct ServoState {
    pub values: [i16; SERVO_COUNT],
    pub selected: usize,
}

impl Default for ServoState {
    fn default() -> Self {
        Self {
            values: [0; SERVO_COUNT],
            selected: 0,
        }
    }
}

impl ServoState {
    pub fn servo_name(index: usize) -> &'static str {
        match index {
            0 => "头部",
            1 => "腰部",
            2 => "左肩膀",
            3 => "左臂",
            4 => "右肩膀",
            5 => "右臂",
            _ => "未知",
        }
    }

    pub fn next_servo(&mut self) {
        self.selected = (self.selected + 1) % SERVO_COUNT;
    }

    pub fn prev_servo(&mut self) {
        self.selected = (self.selected + SERVO_COUNT - 1) % SERVO_COUNT;
    }

    pub fn increase(&mut self) {
        if self.values[self.selected] < SERVO_MAX {
            self.values[self.selected] = (self.values[self.selected] + 1).min(SERVO_MAX);
        }
    }

    pub fn decrease(&mut self) {
        if self.values[self.selected] > SERVO_MIN {
            self.values[self.selected] = (self.values[self.selected] - 1).max(SERVO_MIN);
        }
    }

    pub fn increase_big(&mut self) {
        if self.values[self.selected] < SERVO_MAX {
            self.values[self.selected] = (self.values[self.selected] + 5).min(SERVO_MAX);
        }
    }

    pub fn decrease_big(&mut self) {
        if self.values[self.selected] > SERVO_MIN {
            self.values[self.selected] = (self.values[self.selected] - 5).max(SERVO_MIN);
        }
    }

    pub fn to_percent(value: i16) -> u16 {
        ((value - SERVO_MIN) * 100 / (SERVO_MAX - SERVO_MIN)) as u16
    }

    /// 转换为 JointConfig
    pub fn to_joint_config(&self) -> JointConfig {
        JointConfig {
            enable: 1, // 使能
            angles: self.values.map(|v| v as f32),
            padding: [0u8; 7],
        }
    }
}
