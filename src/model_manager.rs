use hf_hub::api::sync::Api;
use std::collections::HashMap;
use std::path::PathBuf;

pub struct ModelManager {
    // 使用 HashMap 存储
    paths: HashMap<String, PathBuf>,
}

/// 模型注册项
struct ModelEntry {
    key: &'static str,
    repo_id: &'static str,
    filename: &'static str,
}

impl ModelManager {
    /// 初始化并同步所有模型
    pub fn init() -> anyhow::Result<Self> {
        let api = Api::new()?;
        let mut paths = HashMap::new();

        // 配置所有模型
        let registry = vec![
            ModelEntry {
                key: "sense_voice",
                repo_id: "csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17",
                filename: "model.int8.onnx",
            },
            ModelEntry {
                key: "silero_vad",
                repo_id: "deepghs/silero-vad-onnx",
                filename: "silero_vad.onnx",
            },
            ModelEntry {
                key: "qwen",
                repo_id: "Qwen/Qwen2.5-0.5B-Instruct-GGUF",
                filename: "qwen2.5-0.5b-instruct-q4_0.gguf",
            },
            ModelEntry {
                key: "tokenizer",
                repo_id: "onnx-community/Qwen2.5-0.5B-Instruct",
                filename: "tokenizer.json",
            },
            ModelEntry {
                key: "yolo_face",
                repo_id: "deepghs/yolo-face",
                filename: "yolov8n-face/model.onnx",
            },
        ];

        log::info!("--- 正在检查系统模型资源 ---");

        for entry in &registry {
            let repo = api.model(entry.repo_id.to_string());
            log::info!(
                "正在下载 [{}] from {}/{} ...",
                entry.key,
                entry.repo_id,
                entry.filename
            );
            match repo.get(entry.filename) {
                Ok(path) => {
                    log::info!("✓ 资源就绪 [{}]: {:?}", entry.key, path);
                    paths.insert(entry.key.to_string(), path);
                }
                Err(e) => {
                    log::error!("✗ 资源缺失 [{}]: {:?}", entry.key, e);
                }
            }
        }

        log::info!("--- 模型准备完毕 ---\n");
        Ok(ModelManager { paths })
    }

    /// 获取特定模型的路径
    pub fn get(&self, key: &str) -> Option<PathBuf> {
        self.paths.get(key).cloned()
    }
}
