use crate::emotion::parse_mood;
use crate::llm::trait_::LlmTrait;
use crate::llm::LlmResponse;
use anyhow::Result;
use boteyes::Mood;
use candle_core::quantized::gguf_file::Content;
use candle_core::{Device, Tensor};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub mod quantized_qwen2 {
    pub use candle_transformers::models::quantized_qwen2::ModelWeights;
}

pub struct QwenLlm {
    model_path: PathBuf,
    tokenizer: Option<tokenizers::Tokenizer>,
    device: Device,
    model: Option<quantized_qwen2::ModelWeights>,
}

impl QwenLlm {
    pub fn load(model_path: PathBuf) -> Self {
        Self {
            model_path,
            tokenizer: None,
            device: Device::Cpu,
            model: None,
        }
    }

    pub fn load_tokenizer(&mut self, tokenizer_path: impl AsRef<Path>) -> Result<()> {
        let tokenizer = tokenizers::Tokenizer::from_file(tokenizer_path)
            .map_err(|e| anyhow::anyhow!("Failed to load tokenizer: {}", e))?;
        self.tokenizer = Some(tokenizer);
        Ok(())
    }

    pub fn preload(&mut self) -> Result<()> {
        let start = Instant::now();
        let mut file = File::open(&self.model_path)?;
        let gguf = Content::read(&mut file)?;
        let model = quantized_qwen2::ModelWeights::from_gguf(gguf, &mut file, &self.device)?;
        self.model = Some(model);
        log::info!("Model loaded in {:?}", start.elapsed());
        Ok(())
    }

    pub fn analyze_mood(&mut self, user_input: &str) -> Result<Mood> {
        let start = Instant::now();
        let prompt = build_emotion_prompt(user_input);
        let response = self.generate(&prompt, 8)?;
        log::debug!("Response: {:?}, used time: {:?}", response, start.elapsed());
        let mood = parse_mood(&response);
        Ok(mood)
    }

    pub fn generate(&mut self, prompt: &str, max_tokens: usize) -> Result<String> {
        let tokenizer = self
            .tokenizer
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Tokenizer not loaded"))?;
        let model = self
            .model
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Model not preloaded"))?;

        let encoding = tokenizer
            .encode(prompt, true)
            .map_err(|e| anyhow::anyhow!("Encode error: {}", e))?;
        let mut all_tokens: Vec<u32> = encoding.get_ids().to_vec();
        let prompt_len = all_tokens.len();
        let eos_token = tokenizer.token_to_id("<|im_end|>").unwrap_or(151645);

        let input = Tensor::new(all_tokens.as_slice(), &self.device)?.unsqueeze(0)?;
        let logits = model.forward(&input, 0)?;

        let mut next_token = logits.squeeze(0)?.argmax(0)?.to_scalar::<u32>()?;
        all_tokens.push(next_token);

        for i in 0..max_tokens {
            if next_token == eos_token {
                break;
            }
            let input = Tensor::new(&[next_token], &self.device)?.unsqueeze(0)?;
            let logits = model.forward(&input, prompt_len + i)?;
            next_token = logits.squeeze(0)?.argmax(0)?.to_scalar::<u32>()?;
            all_tokens.push(next_token);
        }

        Ok(tokenizer
            .decode(&all_tokens[prompt_len..], true)
            .unwrap_or_default())
    }
}

// 为 QwenLlm 实现 LlmTrait
impl LlmTrait for QwenLlm {
    fn analyze_mood(&mut self, user_input: &str) -> Result<LlmResponse> {
        let mood = QwenLlm::analyze_mood(self, user_input)?;
        Ok(LlmResponse {
            mood,
            actions: Vec::new(),
        })
    }

    fn chat(&mut self, user_input: &str) -> Result<String> {
        let prompt = build_chat_prompt(user_input);
        let text = QwenLlm::generate(self, &prompt, 64)?;
        Ok(text.trim().to_string())
    }
}
fn build_emotion_prompt(user_input: &str) -> String {
    format!(
        "<|im_start|>system\n分析用户输入的情感，只输出情感标签。\n情感选项：开心、难过、生气、困惑、害怕、中性\n输出格式：[情感]<|im_end|>\n<|im_start|>user\n{user_input}\n<|im_end|>\n<|im_start|>assistant\n"
    )
}

/// 桌宠对话 prompt. 简短中文回复 (≤ 30 字), 不要解释, 不要 markdown.
fn build_chat_prompt(user_input: &str) -> String {
    format!(
        "system
你是一个桌面机器人, 用简短中文回复用户 (≤ 30 字). 不要解释, 不要 markdown.
user
{user_input}

assistant
"
    )
}
