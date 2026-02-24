use crate::emotion::parse_mood;
use anyhow::Result;
use boteyes::Mood;
use candle_core::quantized::gguf_file::Content;
use candle_core::{Device, Tensor};
use std::fs::File;
use std::path::Path;
use std::time::Instant;

pub mod quantized_qwen2 {
    pub use candle_transformers::models::quantized_qwen2::ModelWeights;
}

pub struct QwenLlm {
    model_path: String,
    tokenizer: Option<tokenizers::Tokenizer>,
    device: Device,
    model: Option<quantized_qwen2::ModelWeights>,
}

impl QwenLlm {
    pub fn load(model_path: &str) -> Result<Self> {
        let path = Path::new(model_path);
        if !path.exists() {
            anyhow::bail!("Model file not found: {}", model_path);
        }
        Ok(Self {
            model_path: model_path.to_string(),
            tokenizer: None,
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
        log::info!("Response: {:?}, used time: {:?}", response, start.elapsed());
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

fn build_emotion_prompt(user_input: &str) -> String {
    format!(
        "<|im_start|>system\n分析用户输入的情感，只输出情感标签。\n情感选项：开心、难过、生气、困惑、害怕、中性\n输出格式：[情感]<|im_end|>\n<|im_start|>user\n{user_input}\n<|im_end|>\n<|im_start|>assistant\n"
    )
}
