//! LLM 响应结构
//!
//! 用于统一在线和本地 LLM 的返回值，包含情感和可选的舵机动作

use boteyes::Mood;

/// LLM 响应结构
#[derive(Debug, Clone, Default)]
pub struct LlmResponse {
    /// 情感状态
    pub mood: Mood,
    /// 舵机动作列表
    pub actions: Vec<Action>,
}

/// 舵机动作
#[derive(Debug, Clone)]
pub struct Action {
    /// 舵机索引 (0-5)
    pub servo_index: u8,
    /// 目标角度 (i16 支持负角度)
    pub angle: i16,
    /// 动作持续时间（毫秒），用于顺序执行时控制速度
    pub duration_ms: u32,
}

#[allow(dead_code)]
impl Action {
    /// 创建新的动作
    #[must_use] 
    pub fn new(servo_index: u8, angle: i16, duration_ms: u32) -> Self {
        Self {
            servo_index,
            angle,
            duration_ms,
        }
    }

    /// 预设动作：左手挥动
    #[must_use] 
    pub fn wave_hand_left() -> Self {
        Self::new(2, 90, 500)
    }

    /// 预设动作：右手挥动
    #[must_use] 
    pub fn wave_hand_right() -> Self {
        Self::new(4, 90, 500)
    }

    /// 预设动作：点头
    #[must_use] 
    pub fn nod_head() -> Self {
        Self::new(0, 10, 200)
    }

    /// 预设动作：摇头
    #[must_use] 
    pub fn shake_head() -> Self {
        Self::new(0, 15, 200)
    }
}

impl Default for Action {
    fn default() -> Self {
        Self {
            servo_index: 0,
            angle: 0,
            duration_ms: 300,
        }
    }
}
