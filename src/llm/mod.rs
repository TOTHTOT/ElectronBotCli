//! LLM 模块
//!
//! 使用 Candle 加载 GGUF 模型进行推理
//!
//! ## 使用方法
//!
//! ```rust
//! use ele_bot::llm::QwenLlm;
//!
//! // 加载模型
//! let mut llm = QwenLlm::load("assets/module/llm/qwen3-0.6b-rust-sft-q8_0.gguf")?;
//!
//! // 加载分词器 (需要 tokenizer.json)
//! llm.load_tokenizer("assets/module/llm/tokenizer.json")?;
//!
//! // 生成回复
//! let response = llm.generate("你好", 256)?;
//! println!("{}", response);
//! ```
//!
//! ## 文件要求
//!
//! - 模型: `assets/module/llm/Qwen3-0.6B-Q3_K_M.gguf`
//! - 分词器: `assets/module/llm/tokenizer.json` 

pub mod llm;

pub use llm::QwenLlm;
