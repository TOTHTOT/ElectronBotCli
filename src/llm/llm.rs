use anyhow::Result;
use candle_core::{Device, Tensor};
use candle_core::quantized::gguf_file::Content;
use crate::emotion::Emotion;
use std::path::Path;
use std::fs::File;
use std::time::Instant;

const MAX_HISTORY_ROUNDS: usize = 4;

#[derive(Clone)]
struct ChatMessage {
    role: String,
    content: String,
}

pub mod quantized_qwen2 {
    pub use candle_transformers::models::quantized_qwen2::ModelWeights;
}

pub struct QwenLlm {
    model_path: String,
    tokenizer: Option<tokenizers::Tokenizer>,
    history: Vec<ChatMessage>,
    device: Device,
    // 【核心修改】直接持久化存储模型，避免重复创建
    model: Option<quantized_qwen2::ModelWeights>,
}

impl QwenLlm {
    pub fn load(model_path: &str) -> Result<Self> {
        let path = Path::new(model_path);
        if !path.exists() {
            anyhow::bail!("Model file not found: {}", model_path);
        }
        log::info!("Qwen LLM initialized with path: {}", model_path);
        Ok(Self {
            model_path: model_path.to_string(),
            tokenizer: None,
            history: Vec::new(),
            device: Device::Cpu,
            model: None,
        })
    }

    pub fn load_tokenizer(&mut self, tokenizer_path: &str) -> Result<()> {
        let tokenizer = tokenizers::Tokenizer::from_file(tokenizer_path)
            .map_err(|e| anyhow::anyhow!("Failed to load tokenizer: {}", e))?;
        self.tokenizer = Some(tokenizer);
        Ok(())
    }

    /// 【核心修改】预加载模型：只在这里运行一次 ModelWeights::from_gguf
    pub fn preload(&mut self) -> Result<()> {
        let start = Instant::now();
        let mut file = File::open(&self.model_path)?;
        let gguf = Content::read(&mut file)?;

        // 这一步在 RK3566 上最耗时，所以我们只做一次
        let model = quantized_qwen2::ModelWeights::from_gguf(gguf, &mut file, &self.device)?;

        self.model = Some(model);

        // 修改这里：不再访问私有的 tensor_infos
        log::info!("Model weights loaded into memory successfully in {:?}", start.elapsed());
        Ok(())
    }
    fn build_prompt(&self, user_input: &str) -> String {
        let mut prompt = String::new();
        // 只让 LLM 输出情感标签，不需要完整回复
        prompt.push_str("<|im_start|>system\n分析用户输入的情感，只输出情感标签。\n情感选项：开心、难过、生气、惊讶、害怕、中性\n输出格式：[情感]<|im_end|>\n");

        prompt.push_str(&format!("<|im_start|>user\n{}\n<|im_end|>\n<|im_start|>assistant\n", user_input));
        prompt
    }

    /// 解析 LLM 回复中的情感
    fn parse_emotion(response: &str) -> Emotion {
        // 提取情感标签
        for emotion in ["开心", "难过", "生气", "惊讶", "害怕", "中性"] {
            if response.starts_with(&format!("[{}]", emotion)) {
                return match emotion {
                    "开心" => Emotion::Happy,
                    "难过" => Emotion::Sad,
                    "生气" => Emotion::Angry,
                    "惊讶" => Emotion::Surprise,
                    "害怕" => Emotion::Fear,
                    _ => Emotion::Neutral,
                };
            }
        }
        // 如果没有情感标签，使用关键词匹配
        Emotion::from_text(response)
    }

    /// 只分析情感，不生成完整回复
    pub fn analyze_emotion(&mut self, user_input: &str) -> Result<Emotion> {
        let full_prompt = self.build_prompt(user_input);

        // 只生成少量 tokens，快速获取情感
        let response = self.generate(&full_prompt, 8)?;
        let emotion = Self::parse_emotion(&response);

        log::info!("Emotion: {} -> {}", user_input, emotion.description());
        Ok(emotion)
    }

    fn compress_history(&mut self) {
        if self.history.len() >= MAX_HISTORY_ROUNDS * 2 {
            // 每次清理保留最近的对话
            let new_history = self.history.iter().skip(2).cloned().collect();
            self.history = new_history;
        }
    }

    /// 对话并返回（回复，情感）
    pub fn chat(&mut self, user_input: &str, max_tokens: usize) -> Result<(String, Emotion)> {
        let start_time = Instant::now();
        let full_prompt = self.build_prompt(user_input);

        // 调用 generate
        let response = self.generate(&full_prompt, max_tokens)?;

        // 解析情感
        let emotion = Self::parse_emotion(&response);

        self.history.push(ChatMessage { role: "user".to_string(), content: user_input.to_string() });
        self.history.push(ChatMessage { role: "assistant".to_string(), content: response.clone() });

        log::info!("Generation complete. Latency: {} ms", start_time.elapsed().as_millis());
        self.compress_history();
        Ok((response, emotion))
    }

    pub fn generate(&mut self, prompt: &str, max_tokens: usize) -> Result<String> {
        let tokenizer = self.tokenizer.as_ref().ok_or_else(|| anyhow::anyhow!("Tokenizer not loaded"))?;

        // 【核心修改】直接取 model 的可变引用
        let model = self.model.as_mut().ok_or_else(|| anyhow::anyhow!("Model not preloaded"))?;

        let encoding = tokenizer.encode(prompt, true).map_err(|e| anyhow::anyhow!("Encode error: {}", e))?;
        let mut all_tokens: Vec<u32> = encoding.get_ids().to_vec();
        let prompt_len = all_tokens.len();

        let eos_token = tokenizer.token_to_id("<|im_end|>").unwrap_or(151645);

        // 【关键】重置模型的 KV Cache 状态，因为我们要重新处理整个拼接后的 prompt
        // 在 candle-transformers 的 Qwen2 实现中，forward 时传入 pos=0 会自动触发重置逻辑

        // Prefill 阶段
        let input = Tensor::new(all_tokens.as_slice(), &self.device)?.unsqueeze(0)?;
        let logits = model.forward(&input, 0)?; // 从 pos=0 开始处理 prompt

        // 采样第一个 token
        let mut next_token = logits.squeeze(0)?.argmax(0)?.to_scalar::<u32>()?;
        all_tokens.push(next_token);

        // 自回归生成阶段
        for i in 0..max_tokens {
            if next_token == eos_token { break; }

            let input = Tensor::new(&[next_token], &self.device)?.unsqueeze(0)?;
            // 传入当前总长度作为 pos，实现增量推理
            let logits = model.forward(&input, prompt_len + i)?;
            next_token = logits.squeeze(0)?.argmax(0)?.to_scalar::<u32>()?;

            all_tokens.push(next_token);
        }

        let result = tokenizer.decode(&all_tokens[prompt_len..], true).unwrap_or_default();
        Ok(result)
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
    }
}