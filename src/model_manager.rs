use std::collections::HashMap;
use std::path::PathBuf;

pub struct ModelManager {
    paths: HashMap<String, PathBuf>,
}

/// 模型配置: (key, repo_id, filename, rknn_path)
type ModelConfig = (&'static str, &'static str, &'static str, &'static str);

/// 从 HuggingFace 下载模型
fn download_from_hf(
    key: &str,
    repo_id: &str,
    filename: &str,
    api: &hf_hub::api::sync::Api,
) -> Option<PathBuf> {
    let repo = api.model(repo_id.to_string());
    log::info!("正在下载 [{}] from {}/{} ...", key, repo_id, filename);
    match repo.get(filename) {
        Ok(path) => {
            log::info!("✓ 资源就绪 [{}]: {:?}", key, path);
            Some(path)
        }
        Err(e) => {
            log::error!("✗ 资源缺失 [{}]: {:?}", key, e);
            None
        }
    }
}

impl ModelManager {
    /// 初始化并同步所有模型
    pub fn init() -> anyhow::Result<Self> {
        let mut paths = HashMap::new();

        // 模型配置: (key, repo_id, filename, rknn_path)
        let models: Vec<ModelConfig> = vec![
            ("sense_voice", "csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17", "model.int8.onnx", "./model/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/model.int8.rknn"),
            ("silero_vad", "deepghs/silero-vad-onnx", "silero_vad.onnx", ""),
            ("qwen", "Qwen/Qwen2.5-0.5B-Instruct-GGUF", "qwen2.5-0.5b-instruct-q4_0.gguf", ""),
            ("tokenizer", "onnx-community/Qwen2.5-0.5B-Instruct", "tokenizer.json", ""),
            #[cfg(not(all(target_os = "linux", target_arch = "aarch64")))]
            ("yolo_face", "deepghs/yolo-face", "yolov8n-face/model.onnx", ""),
            #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
            ("retinaface_rknn", "ElectronBotCli/retinaface_rknn", "model/RetinaFace.rknn", ""),
            #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
            ("retinaface_test_img", "ElectronBotCli/retinaface_rknn", "model/test.jpg", ""),
        ];

        log::info!("--- 正在检查系统模型资源 ---");

        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        {
            use hf_hub::api::sync::Api;
            let api = Api::new()?;

            for (key, repo, filename, rknn) in &models {
                // 优先使用 rknn 路径, 否则使用 hf 默认路径
                if !rknn.is_empty() && PathBuf::from(rknn).exists() {
                    let path = PathBuf::from(rknn);
                    log::info!("✓ 使用 RKNN 模型 [{}]: {:?}", key, path);
                    paths.insert(key.to_string(), path);
                } else if let Some(path) = download_from_hf(key, repo, filename, &api) {
                    log::info!("✓ 使用 ONNX 模型 [{}]: {:?}", key, path);
                    paths.insert(key.to_string(), path);
                }
            }
        }

        #[cfg(not(all(target_os = "linux", target_arch = "aarch64")))]
        {
            use hf_hub::api::sync::Api;
            let api = Api::new()?;

            for (key, repo, filename, _rknn) in &models {
                if let Some(path) = download_from_hf(key, repo, filename, &api) {
                    paths.insert(key.to_string(), path);
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
