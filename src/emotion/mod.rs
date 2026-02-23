use boteyes::Mood;

/// LLM 返回的情感标签直接映射到 Mood
pub fn parse_mood(llm_output: &str) -> Mood {
    for (label, mood) in &[
        ("[开心]", Mood::Happy),
        ("[难过]", Mood::Sad),
        ("[生气]", Mood::Angry),
        ("[惊讶]", Mood::Surprise),
        ("[害怕]", Mood::Confuse),
    ] {
        if llm_output.starts_with(label) {
            return *mood;
        }
    }
    Mood::Default
}
