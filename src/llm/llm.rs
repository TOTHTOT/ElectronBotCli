use anyhow::Result;
use candle_core::{Device, Tensor};
use candle_core::quantized::gguf_file::Content;
use std::path::Path;
use std::fs::File;
use std::io::Read;
use std::time::Instant;

const MAX_HISTORY_ROUNDS: usize = 4;

#[derive(Clone)]
struct ChatMessage {
    role: String,
    content: String,
}

/// GGUF 文件内容缓存
struct GgufCache {
    data: Vec<u8>,
}

impl GgufCache {
    fn new(path: &str) -> Result<Self> {
        let mut file = File::open(path)?;
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;
        log::info!("Loaded GGUF file into memory: {} bytes", data.len());
        Ok(Self { data })
    }

    fn create_model(&self, device: &Device) -> Result<quantized_qwen2::ModelWeights> {
        use candle_transformers::models::quantized_qwen2::ModelWeights;
        use std::io::Cursor;

        let mut cursor = Cursor::new(&self.data);
        let gguf = Content::read(&mut cursor)?;
        log::info!("Loaded {} tensors from GGUF", gguf.tensor_infos.len());

        let model = ModelWeights::from_gguf(gguf, &mut cursor, device)?;
        Ok(model)
    }
}

pub struct QwenLlm {
    model_path: String,
    tokenizer: Option<tokenizers::Tokenizer>,
    history: Vec<ChatMessage>,
    device: Device,
    gguf_cache: Option<GgufCache>,
}

pub mod quantized_qwen2 {
    pub use candle_transformers::models::quantized_qwen2::ModelWeights;
}

impl QwenLlm {
    pub fn load(model_path: &str) -> Result<Self> {
        let path = Path::new(model_path);
        if !path.exists() {
            anyhow::bail!("Model file not found: {}", model_path);
        }
        log::info!("Qwen GGUF model: {}", model_path);
        Ok(Self {
            model_path: model_path.to_string(),
            tokenizer: None,
            history: Vec::new(),
            device: Device::Cpu,
            gguf_cache: None,
        })
    }

    pub fn load_tokenizer(&mut self, tokenizer_path: &str) -> Result<()> {
        let tokenizer = tokenizers::Tokenizer::from_file(tokenizer_path)
            .map_err(|e| anyhow::anyhow!("Failed to load tokenizer: {}", e))?;
        self.tokenizer = Some(tokenizer);
        Ok(())
    }

    /// 预加载模型到内存（加载 GGUF 文件）
    pub fn preload(&mut self) -> Result<()> {
        self.gguf_cache = Some(GgufCache::new(&self.model_path)?);
        Ok(())
    }

    fn build_prompt(&self, user_input: &str) -> String {
        let mut prompt = String::new();

        // 添加系统提示，让模型更好地理解上下文
        prompt.push_str("<|im_start|>system\n你是一个友好的AI助手，会记住对话内容。<|im_end|>\n");

        for msg in &self.history {
            if msg.role == "user" {
                prompt.push_str(&format!("<|im_start|>user\n{}\n<|im_end|>\n", msg.content));
            } else if msg.role == "assistant" {
                prompt.push_str(&format!("<|im_start|>assistant\n{}\n<|im_end|>\n", msg.content));
            }
        }

        prompt.push_str(&format!(
            "<|im_start|>user\n{}\n<|im_end|>\n<|im_start|>assistant\n",
            user_input
        ));

        prompt
    }

    fn compress_history(&mut self) {
        if self.history.len() >= MAX_HISTORY_ROUNDS * 2 {
            let keep_count = MAX_HISTORY_ROUNDS;
            let new_history: Vec<ChatMessage> =
                self.history.iter().skip(keep_count).cloned().collect();
            self.history = new_history;
        }
    }

    /// 进行一轮对话, 会保存记录到历史
    pub fn chat(&mut self, user_input: &str, max_tokens: usize) -> Result<String> {
        let start_time = Instant::now();
        let full_prompt = self.build_prompt(user_input);
        let response = self.generate(&full_prompt, max_tokens)?;

        self.history.push(ChatMessage {
            role: "user".to_string(),
            content: user_input.to_string(),
        });
        self.history.push(ChatMessage {
            role: "assistant".to_string(),
            content: response.clone(),
        });

        log::info!("current generate total use:{} ms", start_time.elapsed().as_millis());
        self.compress_history();
        Ok(response)
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    pub fn generate(&mut self, prompt: &str, max_tokens: usize) -> Result<String> {
        // 从缓存创建新模型实例
        let cache = self.gguf_cache.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Model not preloaded, call preload() first"))?;
        let mut model = cache.create_model(&self.device)?;

        let tokenizer = match &self.tokenizer {
            Some(t) => t,
            None => anyhow::bail!("Tokenizer not loaded"),
        };

        let encoding = tokenizer
            .encode(prompt, true)
            .map_err(|e| anyhow::anyhow!("Encode error: {}", e))?;
        let mut all_tokens: Vec<u32> = encoding.get_ids().to_vec();
        let prompt_len = all_tokens.len();

        // Qwen2 使用 <|im_end|> 作为 EOS
        let eos_token = tokenizer.token_to_id("<|im_end|>").unwrap_or(151645);

        // 初始化 KV cache
        let input = Tensor::new(all_tokens.as_slice(), &self.device)?.unsqueeze(0)?;
        let _ = model.forward(&input, prompt_len)?;

        // 自回归生成
        for _ in 0..max_tokens {
            let next_token = *all_tokens.last().unwrap();
            let input = Tensor::new(&[next_token], &self.device)?.unsqueeze(0)?;

            let logits = model.forward(&input, all_tokens.len())?;
            let logits = logits.squeeze(0)?;

            let next_token = logits.argmax(0)?.to_scalar::<u32>()?;
            all_tokens.push(next_token);

            if next_token == eos_token {
                break;
            }
            // 果生成了太多token，强制停止
            if all_tokens.len() > prompt_len + max_tokens {
                break;
            }
        }

        let generated_tokens = &all_tokens[prompt_len..];
        let result = tokenizer.decode(generated_tokens, true).unwrap_or_default();

        Ok(result)
    }
}
