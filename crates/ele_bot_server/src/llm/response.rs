//! LLM 响应结构
//!
//! 情感/动作分析 (`analyze_mood`) 的返回值与解析逻辑: 情感标签 +
//! 可选的舵机动作 JSON, 由 zeroclaw mood session 的单轮回复解析而来

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

/// 情感分析的指令文本 (作为 mood session 单轮 prompt 的指令部分)
///
/// 明确要求输出格式与 `parse_mood` 的解析逻辑一致
/// (`parse_mood` 使用 starts_with("[开心]") 等方式匹配)
#[must_use]
pub fn system_prompt() -> &'static str {
    r#"你是一个情感分析助手，同时负责生成机器人动作。

## 情感输出（必须）
根据用户输入判断情感，只输出情感标签：
- 开心：[开心]
- 难过：[难过]
- 生气：[生气]
- 惊讶：[惊讶]
- 困惑：[困惑]
- 中性或其他：[中性]

## 动作输出（可选）
根据用户输入生成动作，格式为JSON数组。
**重要：动作必须包含完整的流程（抬起→执行→放下），每个动作完成后要恢复到0度。**

### 舵机对应关系
- 0: 头部 (范围: -15° ~ 15°)
- 1: 左肩 (范围: -30° ~ 30°)
- 2: 左臂 (范围: -180° ~ 180°)
- 3: 右肩 (范围: -30° ~ 30°)
- 4: 右臂 (范围: -180° ~ 180°)
- 5: 身体 (范围: -90° ~ 90°)

### 完整动作示例
1. 右手挥手：抬起(90°) → 摆动1(60°) → 摆动2(90°) → 摆动3(60°) → 放下(0°)
   [{"servo": 4, "angle": 90, "duration": 200}, {"servo": 4, "angle": 60, "duration": 150}, {"servo": 4, "angle": 90, "duration": 150}, {"servo": 4, "angle": 60, "duration": 150}, {"servo": 4, "angle": 0, "duration": 200}]
2. 点头：低头(10°) → 抬起(-10°) → 恢复(0°)
   [{"servo": 0, "angle": 10, "duration": 150}, {"servo": 0, "angle": -10, "duration": 150}, {"servo": 0, "angle": 0, "duration": 150}]
3. 摇头：左转(15°) → 右转(-15°) → 恢复(0°)
   [{"servo": 0, "angle": 15, "duration": 150}, {"servo": 0, "angle": -15, "duration": 150}, {"servo": 0, "angle": 0, "duration": 150}]
4. 身体左转：左转(45°) → 恢复(0°)
   [{"servo": 5, "angle": 45, "duration": 300}, {"servo": 5, "angle": 0, "duration": 300}]

## 输出格式
先输出情感标签，换行后输出动作JSON（无可用动作时输出 []）：
[情感]
[{"servo": X, "angle": Y, "duration": Z}, ...]
"#
}

/// 分离 LLM 输出中的情感标签和动作 JSON
#[must_use]
pub fn split_response(content: &str) -> (&str, &str) {
    // zeroclaw / MiniMax 常在正文前输出若干空行: 先剥掉前导空白再切分.
    // 第一个非空行 = 情感标签; 其后剩余部分 = 动作 JSON (允许跨多行).
    let body = content.trim_start();
    if let Some(nl) = body.find('\n') {
        (body[..nl].trim(), body[nl + 1..].trim())
    } else if body.contains('[') && body.contains("servo") {
        ("[中性]", body.trim())
    } else {
        (body.trim(), "[]")
    }
}

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
        return actions.into_iter().map(ActionJson::into_action).collect();
    }

    // 如果直接解析失败，尝试提取 JSON 部分（处理可能的前缀文本）
    if let Some(start) = json_str.find('[') {
        if let Some(end) = json_str.rfind(']') {
            let json_part = &json_str[start..=end];
            if let Ok(actions) = serde_json::from_str::<Vec<ActionJson>>(json_part) {
                return actions.into_iter().map(ActionJson::into_action).collect();
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

impl ActionJson {
    fn into_action(self) -> Action {
        Action {
            servo_index: self.servo.clamp(0, 5),
            angle: self.angle.clamp(-180_i32, 180_i32) as i16,
            duration_ms: self.duration.clamp(100, 5000),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_prompt_format() {
        let prompt = system_prompt();
        // 验证 prompt 中包含了正确的情感标签格式
        assert!(prompt.contains("[开心]"));
        assert!(prompt.contains("[难过]"));
        assert!(prompt.contains("[生气]"));
    }

    #[test]
    fn test_split_response_basic() {
        let (mood, actions) =
            split_response("[开心]\n[{\"servo\": 4, \"angle\": 90, \"duration\": 200}]");
        assert_eq!(mood, "[开心]");
        assert_eq!(
            actions,
            "[{\"servo\": 4, \"angle\": 90, \"duration\": 200}]"
        );
    }

    #[test]
    fn test_split_response_leading_blank_lines() {
        // MiniMax / zeroclaw 常在正文前输出 \n\n: 前导空行不能吃掉情感和动作
        let (mood, actions) =
            split_response("\n\n[开心]\n[{\"servo\": 4, \"angle\": 90, \"duration\": 200}]");
        assert_eq!(mood, "[开心]");
        assert!(actions.starts_with('['));
        assert_eq!(parse_actions(actions).len(), 1);
    }

    #[test]
    fn test_split_response_multiline_actions_json() {
        let (mood, actions) =
            split_response("[难过]\n[\n  {\"servo\": 0, \"angle\": 10, \"duration\": 150}\n]");
        assert_eq!(mood, "[难过]");
        assert_eq!(parse_actions(actions).len(), 1);
    }

    #[test]
    fn test_split_response_single_line_mood_only() {
        let (mood, actions) = split_response("[中性]");
        assert_eq!(mood, "[中性]");
        assert_eq!(actions, "[]");
    }
}
