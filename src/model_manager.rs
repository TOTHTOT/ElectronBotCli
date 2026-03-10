use std::collections::HashMap;
use std::path::PathBuf;

pub struct ModelManager {
    paths: HashMap<String, PathBuf>,
}

impl ModelManager {
    /// 初始化并同步所有模型
    pub fn init() -> anyhow::Result<Self> {
        let mut paths = HashMap::new();

        // 模型配置: (key, repo_id, filename, rknn_path)
        let models = vec![
            ("sense_voice", "csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17", "model.int8.onnx", "/home/radxa/model/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/model.int8.rknn"),
            ("silero_vad", "deepghs/silero-vad-onnx", "silero_vad.onnx", "/home/radxa/model/deepghs/silero-vad-onnx/silero_vad.rknn"),
            ("qwen", "Qwen/Qwen2.5-0.5B-Instruct-GGUF", "qwen2.5-0.5b-instruct-q4_0.gguf", ""),
            ("tokenizer", "onnx-community/Qwen2.5-0.5B-Instruct", "tokenizer.json", ""),
            ("yolo_face", "deepghs/yolo-face", "yolov8n-face/model.onnx", "/home/radxa/model/deepghs/yolo-face/yolo_face.rknn"),
        ];

        log::info!("--- 正在检查系统模型资源 ---");

        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        {
            for (key, _repo, _file, rknn) in &models {
                if !rknn.is_empty() {
                    let path = PathBuf::from(rknn);
                    if path.exists() {
                        log::info!("✓ 使用 RKNN 模型 [{}]: {:?}", key, path);
                        paths.insert(key.to_string(), path);
                    } else {
                        log::warn!("✗ RKNN 模型不存在 [{}]: {:?}", key, path);
                    }
                }
            }
        }

        #[cfg(not(all(target_os = "linux", target_arch = "aarch64")))]
        {
            use hf_hub::api::sync::Api;
            let api = Api::new()?;
            for (key, repo, file, _rknn) in &models {
                // 下载模型
                let repo_id = *repo;
                let repo = api.model(repo.to_string());
                log::info!("正在下载 [{}] from {}/{} ...", key, repo_id, file);
                match repo.get(file) {
                    Ok(path) => {
                        log::info!("✓ 资源就绪 [{}]: {:?}", key, path);
                        paths.insert(key.to_string(), path);
                    }
                    Err(e) => log::error!("✗ 资源缺失 [{}]: {:?}", key, e),
                }
            }
        }

        log::info!("--- 模型准备完毕 ---\n");
        Ok(ModelManager { paths })
    }

    /// 获取模型路径
    pub fn get(&self, key: &str) -> Option<PathBuf> {
        self.paths.get(key).cloned()
    }
}
