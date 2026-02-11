//! Joint 关节控制模块
//!
//! 提供 6 个舵机的角度控制和数据序列化

pub const SERVO_COUNT: usize = 6;

// 舵机配置结构体
struct ServoConfig {
    name: &'static str,
    min: i16,
    max: i16,
}

// 各舵机配置
const SERVOS: [ServoConfig; SERVO_COUNT] = [
    ServoConfig {
        name: "头部",
        min: -15,
        max: 15,
    },
    ServoConfig {
        name: "左肩",
        min: -30,
        max: 30,
    },
    ServoConfig {
        name: "左臂",
        min: -180,
        max: 180,
    },
    ServoConfig {
        name: "右肩",
        min: -30,
        max: 30,
    },
    ServoConfig {
        name: "右臂",
        min: -180,
        max: 180,
    },
    ServoConfig {
        name: "身体",
        min: -90,
        max: 90,
    },
];

// ==================== JointConfig ====================

/// 关节配置数据结构
///
/// 包含使能标志和 6 个舵机角度，序列化为 32 字节
#[derive(Clone, Copy, Debug)]
pub struct JointConfig {
    pub enable: u8,
    pub angles: [f32; SERVO_COUNT],
}

impl Default for JointConfig {
    fn default() -> Self {
        Self {
            enable: 0,
            angles: [0.0; SERVO_COUNT],
        }
    }
}

impl JointConfig {
    /// 转换为 32 字节格式
    pub fn as_bytes(self) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        bytes[0] = self.enable;
        for i in 0..SERVO_COUNT {
            let b = self.angles[i].to_le_bytes();
            bytes[1 + i * 4..1 + i * 4 + 4].copy_from_slice(&b);
        }
        bytes
    }
}

// ==================== ServoState ====================

/// 舵机状态（UI 显示用）
#[derive(Clone, Debug, Default)]
pub struct ServoState {
    pub values: [i16; SERVO_COUNT],
    pub selected: usize,
}

#[allow(dead_code)]
impl ServoState {
    /// 获取舵机名称
    pub fn name(index: usize) -> &'static str {
        SERVOS.get(index).map(|s| s.name).unwrap_or("Unknown")
    }

    /// 获取舵机最小角度
    pub fn min_angle(index: usize) -> i16 {
        SERVOS.get(index).map(|s| s.min).unwrap_or(-125)
    }

    /// 获取舵机最大角度
    pub fn max_angle(index: usize) -> i16 {
        SERVOS.get(index).map(|s| s.max).unwrap_or(125)
    }

    /// 获取舵机范围字符串
    pub fn range_str(index: usize) -> String {
        let s = SERVOS.get(index).unwrap();
        format!("{}° ~ {}°", s.min, s.max)
    }

    /// 选择下一个舵机
    pub fn next(&mut self) {
        self.selected = (self.selected + 1) % SERVO_COUNT;
    }

    /// 选择上一个舵机
    pub fn prev(&mut self) {
        self.selected = (self.selected + SERVO_COUNT - 1) % SERVO_COUNT;
    }

    /// 增加当前舵机角度
    pub fn increase(&mut self) {
        let max = Self::max_angle(self.selected);
        self.values[self.selected] = (self.values[self.selected] + 1).min(max);
    }

    /// 减少当前舵机角度
    pub fn decrease(&mut self) {
        let min = Self::min_angle(self.selected);
        self.values[self.selected] = (self.values[self.selected] - 1).max(min);
    }

    /// 转换为 JointConfig
    pub fn as_config(&self) -> JointConfig {
        JointConfig {
            enable: 1,
            angles: self.values.map(|x| x as f32),
        }
    }
}

// ==================== Joint 控制器 ====================

/// 关节控制器
///
/// 管理所有舵机的状态和配置
#[derive(Debug, Default)]
pub struct Joint {
    state: ServoState,
}

#[allow(dead_code)]
impl Joint {
    /// 创建新的关节控制器
    pub fn new() -> Self {
        Self::default()
    }

    /// 获取所有舵机值
    pub fn values(&self) -> &[i16; SERVO_COUNT] {
        &self.state.values
    }

    /// 获取当前选中的舵机索引
    pub fn selected(&self) -> usize {
        self.state.selected
    }

    /// 切换到下一个舵机
    pub fn next_servo(&mut self) {
        self.state.next();
    }

    /// 切换到上一个舵机
    pub fn prev_servo(&mut self) {
        self.state.prev();
    }

    /// 增加当前舵机角度
    pub fn increase(&mut self) {
        self.state.increase();
    }

    /// 减少当前舵机角度
    pub fn decrease(&mut self) {
        self.state.decrease();
    }

    /// 获取当前关节配置
    pub fn config(&self) -> JointConfig {
        self.state.as_config()
    }
}
