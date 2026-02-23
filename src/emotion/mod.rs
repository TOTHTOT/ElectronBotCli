//! 情感模块 - 根据文本内容分析情感并做出反应
//!
//! 1. 情感分析 - 基于关键词匹配
//! 2. 表情控制 - 改变 LCD 表情
//! 3. 声音反馈 - 发出 bibi 声

use boteyes::Mood;

/// 情感类型
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Emotion {
    Happy,    // 开心
    Sad,     // 难过
    Angry,   // 生气
    Surprise, // 惊讶
    Fear,    // 害怕
    Neutral, // 中性
}

/// 情感对应的声音配置
pub struct EmotionSound {
    pub beep_count: u32,    // 声音次数
    pub frequency: f32,     // 音调 (Hz)
    pub duration_ms: u32,   // 每次声音时长 (ms)
    pub interval_ms: u32,   // 声音间隔 (ms)
}

impl Emotion {
    /// 根据文本内容分析情感
    pub fn from_text(text: &str) -> Self {
        let lower = text.to_lowercase();

        // 开心关键词
        let happy_words = ["开心", "高兴", "好", "棒", "喜欢", "爱", "谢谢", "哈哈", "你好", "hello", "hi", "好耶", "太好了", "完美", "赞", "good", "great", "nice"];
        for word in happy_words {
            if lower.contains(word) {
                return Emotion::Happy;
            }
        }

        // 生气/愤怒关键词
        let angry_words = ["生气", "愤怒", "讨厌", "滚", "傻", "笨", "烦", "去你的", "可恶", "shit", "fuck", "damn"];
        for word in angry_words {
            if lower.contains(word) {
                return Emotion::Angry;
            }
        }

        // 难过关键词
        let sad_words = ["难过", "伤心", "哭", "难受", "痛苦", "郁闷", "不爽", "累", "困", "sad", "crying"];
        for word in sad_words {
            if lower.contains(word) {
                return Emotion::Sad;
            }
        }

        // 惊讶关键词
        let surprise_words = ["哇", "啊", "哦", "惊讶", "震惊", "真的", "不会吧", "真假", "wow", "oh", "really"];
        for word in surprise_words {
            if lower.contains(word) {
                return Emotion::Surprise;
            }
        }

        // 害怕关键词
        let fear_words = ["怕", "恐怖", "吓人", "不敢", "好怕", "scary", "afraid"];
        for word in fear_words {
            if lower.contains(word) {
                return Emotion::Fear;
            }
        }

        Emotion::Neutral
    }

    /// 转换为 boteyes 的 Mood
    pub fn to_mood(&self) -> Mood {
        match self {
            Emotion::Happy => Mood::Happy,
            Emotion::Sad => Mood::Angry,
            Emotion::Angry => Mood::Angry,
            Emotion::Surprise => Mood::Happy,
            Emotion::Fear => Mood::Angry,
            Emotion::Neutral => Mood::Default,
        }
    }

    /// 获取情感对应的声音配置
    pub fn sound(&self) -> EmotionSound {
        match self {
            // 开心: 高音，短促两声
            Emotion::Happy => EmotionSound {
                beep_count: 2,
                frequency: 800.0,
                duration_ms: 100,
                interval_ms: 150,
            },
            // 难过: 低沉，一声
            Emotion::Sad => EmotionSound {
                beep_count: 1,
                frequency: 300.0,
                duration_ms: 300,
                interval_ms: 0,
            },
            // 生气: 中高音，急促三声
            Emotion::Angry => EmotionSound {
                beep_count: 3,
                frequency: 500.0,
                duration_ms: 80,
                interval_ms: 100,
            },
            // 惊讶: 高音，两声
            Emotion::Surprise => EmotionSound {
                beep_count: 2,
                frequency: 700.0,
                duration_ms: 120,
                interval_ms: 100,
            },
            // 害怕: 低沉，两声
            Emotion::Fear => EmotionSound {
                beep_count: 2,
                frequency: 400.0,
                duration_ms: 150,
                interval_ms: 200,
            },
            // 中性: 中音，一声
            Emotion::Neutral => EmotionSound {
                beep_count: 1,
                frequency: 440.0,
                duration_ms: 150,
                interval_ms: 0,
            },
        }
    }

    /// 获取情感描述
    pub fn description(&self) -> &str {
        match self {
            Emotion::Happy => "开心",
            Emotion::Sad => "难过",
            Emotion::Angry => "生气",
            Emotion::Surprise => "惊讶",
            Emotion::Fear => "害怕",
            Emotion::Neutral => "平静",
        }
    }
}
