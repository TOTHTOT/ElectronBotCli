use boteyes::Mood;

use crate::llm::response::Action;

/// LLM 返回的情感标签直接映射到 Mood
#[must_use]
pub fn parse_mood(llm_output: &str) -> Mood {
    for (label, mood) in &[
        ("[开心]", Mood::Happy),
        ("[难过]", Mood::Sad),
        ("[生气]", Mood::Angry),
        ("[惊讶]", Mood::Surprise),
        ("[困惑]", Mood::Confuse),
    ] {
        if llm_output.starts_with(label) {
            return *mood;
        }
    }
    Mood::Default
}

/// 解析动作 JSON 字符串
///
/// 期望格式：[{"servo": 0-5, "angle": -180-180, "duration": 100-5000}, ...]
#[must_use]
pub fn parse_actions(json_str: &str) -> Vec<Action> {
    let json_str = json_str.trim();

    // 尝试直接解析 JSON 数组
    if let Ok(actions) = serde_json::from_str::<Vec<ActionJson>>(json_str) {
        return actions
            .into_iter()
            .map(|a| Action {
                servo_index: a.servo.clamp(0, 5),
                angle: a.angle.clamp(-180_i32, 180_i32) as i16,
                duration_ms: a.duration.clamp(100, 5000),
            })
            .collect();
    }

    // 如果直接解析失败，尝试提取 JSON 部分（处理可能的前缀文本）
    if let Some(start) = json_str.find('[') {
        if let Some(end) = json_str.rfind(']') {
            let json_part = &json_str[start..=end];
            if let Ok(actions) = serde_json::from_str::<Vec<ActionJson>>(json_part) {
                return actions
                    .into_iter()
                    .map(|a| Action {
                        servo_index: a.servo.clamp(0, 5),
                        angle: a.angle.clamp(-180_i32, 180_i32) as i16,
                        duration_ms: a.duration.clamp(100, 5000),
                    })
                    .collect();
            }
        }
    }

    Vec::new()
}

#[derive(serde::Deserialize)]
struct ActionJson {
    servo: u8,
    angle: i32, // 使用 i32 支持负角度
    duration: u32,
}
