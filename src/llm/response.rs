//! LLM 响应结构
//!
//! 用于统一在线和本地 LLM 的返回值，包含情感和可选的舵机动作

use boteyes::Mood;

/// LLM 响应结构
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct LlmResponse {
    /// 情感状态
    pub mood: Mood,
    /// 舵机动作列表
    pub actions: Vec<Action>,
}

/// 舵机动作
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Action {
    /// 舵机索引 (0-5)
    pub servo_index: u8,
    /// 目标角度 (0-180)
    pub angle: u8,
}
