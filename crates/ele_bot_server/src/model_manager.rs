use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

pub struct ModelManager {
    paths: HashMap<String, PathBuf>,
}

/// 全局 `ModelManager` 实例 - 懒加载
static MODEL_MANAGER: OnceLock<ModelManager> = OnceLock::new();

/// 模型配置: (key, `repo_id`, filename, `rknn_path`)
type ModelConfig = (&'static str, &'static str, &'static str, &'static str);

/// 从 `HuggingFace` 下载模型
fn download_from_hf(
    key: &str,
    repo_id: &str,
    filename: &str,
    client: &hf_hub::HFClientSync,
) -> Option<PathBuf> {
    let (owner, name) = repo_id.split_once('/')?;
    let repo = client.model(owner, name);
    log::info!("正在下载 [{key}] from {repo_id}/{filename} ...");
    // 离线优先: 设备常在弱网/无外网环境, 先查本地 HF 缓存,
    // 未命中再走在线下载 (hf-hub 1.0 默认在线优先, 弱网下会在
    // 元数据校验上反复重试卡死初始化).
    let result = match repo
        .download_file()
        .filename(filename)
        .local_files_only(true)
        .send()
    {
        Ok(path) => Ok(path),
        Err(_) => repo.download_file().filename(filename).send(),
    };
    match result {
        Ok(path) => {
            log::info!("✓ 资源就绪 [{key}]: {path:?}");
            Some(path)
        }
        Err(e) => {
            log::error!("✗ 资源缺失 [{key}]: {e:?}");
            None
        }
    }
}

impl ModelManager {
    /// 获取全局 `ModelManager` 实例（懒初始化）
    pub fn global() -> &'static ModelManager {
        MODEL_MANAGER.get_or_init(|| Self::init().expect("Failed to initialize ModelManager"))
    }

    /// 初始化并同步所有模型
    pub fn init() -> anyhow::Result<Self> {
        let mut paths = HashMap::new();

        // 模型配置: (key, repo_id, filename, rknn_path)
        let models: Vec<ModelConfig> = vec![
            ("sense_voice", "csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17", "model.int8.onnx", "./model/sherpa-onnx-rk3566-5-seconds-sense-voice-zh-en-ja-ko-yue-2024-07-17/model.rknn"),
            ("sense_voice_tokens", "csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17", "tokens.txt", ""),
            ("silero_vad", "deepghs/silero-vad-onnx", "silero_vad.onnx", ""),
            // VITS TTS 模型 (中文) - 使用 HuggingFace
            ("vits_tts", "csukuangfj/sherpa-onnx-vits-zh-ll", "model.onnx", ""),
            ("vits_tts_tokens", "csukuangfj/sherpa-onnx-vits-zh-ll", "tokens.txt", ""),
            ("vits_tts_lexicon", "csukuangfj/sherpa-onnx-vits-zh-ll", "lexicon.txt", ""),
            // 注意: Kokoro TTS 中文模型不在 HuggingFace, 仅在 GitHub releases
            // 如需 Kokoro, 可从 https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/kokoro-multi-lang-v1_0.tar.bz2 下载
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
            use hf_hub::HFClientSync;
            let api = HFClientSync::new()?;

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
            use hf_hub::HFClientSync;
            let api = HFClientSync::new()?;

            for (key, repo, filename, _rknn) in &models {
                if let Some(path) = download_from_hf(key, repo, filename, &api) {
                    paths.insert((*key).to_string(), path);
                }
            }
        }

        log::info!("--- 模型准备完毕 ---\n");
        Ok(ModelManager { paths })
    }

    /// 获取模型路径
    #[must_use]
    pub fn get(&self, key: &str) -> Option<PathBuf> {
        self.paths.get(key).cloned()
    }
}
