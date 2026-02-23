use boteyes::Mood;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Emotion {
    Happy,
    Sad,
    Angry,
    Surprise,
    Fear,
    Neutral,
}

pub struct EmotionSound {
    pub beep_count: u32,
    pub frequency: f32,
    pub duration_ms: u32,
    pub interval_ms: u32,
}

impl Emotion {
    pub fn from_text(text: &str) -> Self {
        let lower = text.to_lowercase();

        if contains_any(&lower, &["开心", "高兴", "好", "棒", "喜欢", "爱", "谢谢", "哈哈", "你好", "hello", "hi", "好耶", "太好了", "完美", "赞", "good", "great", "nice"]) {
            return Emotion::Happy;
        }
        if contains_any(&lower, &["生气", "愤怒", "讨厌", "滚", "傻", "笨", "烦", "去你的", "可恶", "shit", "fuck", "damn"]) {
            return Emotion::Angry;
        }
        if contains_any(&lower, &["难过", "伤心", "哭", "难受", "痛苦", "郁闷", "不爽", "累", "困", "sad", "crying"]) {
            return Emotion::Sad;
        }
        if contains_any(&lower, &["哇", "啊", "哦", "惊讶", "震惊", "真的", "不会吧", "真假", "wow", "oh", "really"]) {
            return Emotion::Surprise;
        }
        if contains_any(&lower, &["怕", "恐怖", "吓人", "不敢", "好怕", "scary", "afraid"]) {
            return Emotion::Fear;
        }

        Emotion::Neutral
    }

    pub fn to_mood(&self) -> Mood {
        match self {
            Emotion::Happy => Mood::Happy,
            Emotion::Sad | Emotion::Angry | Emotion::Fear => Mood::Angry,
            Emotion::Surprise => Mood::Happy,
            Emotion::Neutral => Mood::Default,
        }
    }

    pub fn sound(&self) -> EmotionSound {
        match self {
            Emotion::Happy => EmotionSound { beep_count: 2, frequency: 800.0, duration_ms: 100, interval_ms: 150 },
            Emotion::Sad => EmotionSound { beep_count: 1, frequency: 300.0, duration_ms: 300, interval_ms: 0 },
            Emotion::Angry => EmotionSound { beep_count: 3, frequency: 500.0, duration_ms: 80, interval_ms: 100 },
            Emotion::Surprise => EmotionSound { beep_count: 2, frequency: 700.0, duration_ms: 120, interval_ms: 100 },
            Emotion::Fear => EmotionSound { beep_count: 2, frequency: 400.0, duration_ms: 150, interval_ms: 200 },
            Emotion::Neutral => EmotionSound { beep_count: 1, frequency: 440.0, duration_ms: 150, interval_ms: 0 },
        }
    }

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

fn contains_any(text: &str, words: &[&str]) -> bool {
    words.iter().any(|w| text.contains(w))
}
