use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

/// 模型文件配置
struct ModelConfig {
    name: &'static str,
    dir: &'static str,
    files: &'static [ModelFile],
}

struct ModelFile {
    name: &'static str,
    url: &'static str,
}

fn get_models() -> Vec<ModelConfig> {
    vec![
        ModelConfig {
            name: "SenseVoice (ASR)",
            dir: "assets/module/sherpa-onnx-sense-voice",
            files: &[
                ModelFile {
                    name: "model.int8.onnx",
                    url: "https://huggingface.co/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/resolve/main/model.int8.onnx",
                },
                ModelFile {
                    name: "tokens.txt",
                    url: "https://huggingface.co/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/resolve/main/tokens.txt",
                },
            ],
        },
        ModelConfig {
            name: "VITS (TTS)",
            dir: "assets/module/vits",
            files: &[
                ModelFile {
                    name: "model.onnx",
                    url: "https://huggingface.co/csukuangfj/vits-ljs/resolve/main/vits-ljs.onnx",
                },
                ModelFile {
                    name: "tokens.txt",
                    url: "https://huggingface.co/csukuangfj/vits-ljs/resolve/main/tokens.txt",
                },
                ModelFile {
                    name: "lexicon.txt",
                    url: "https://huggingface.co/csukuangfj/vits-ljs/resolve/main/lexicon.txt",
                },
            ],
        },
    ]
}

fn main() {
    // 获取项目根目录
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let project_root = Path::new(&manifest_dir);

    // 检查是否需要下载模型
    let download_models = should_download_models();

    if download_models {
        println!("cargo:rerun-if-changed=assets/");
        download_all_models(project_root);
    } else {
        check_models_exist(project_root);
    }
}

/// 判断是否需要下载模型
/// 可以通过环境变量 MODEL_AUTO_DOWNLOAD=1 来启用自动下载
fn should_download_models() -> bool {
    // 检查环境变量
    if env::var("MODEL_AUTO_DOWNLOAD").unwrap_or_default() == "1" {
        return true;
    }

    // 检查是否有任何模型文件缺失
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let project_root = Path::new(&manifest_dir);
    let models = get_models();

    for model in &models {
        let model_dir = project_root.join(model.dir);
        for file in model.files {
            let file_path = model_dir.join(file.name);
            if !file_path.exists() {
                return true;
            }
        }
    }

    false
}

/// 检查模型是否存在，如果缺失则警告
fn check_models_exist(project_root: &Path) {
    let mut missing = Vec::new();
    let models = get_models();

    for model in &models {
        let model_dir = project_root.join(model.dir);
        for file in model.files {
            let file_path = model_dir.join(file.name);
            if !file_path.exists() {
                missing.push((model.name, file.name, file.url));
            }
        }
    }

    if !missing.is_empty() {
        println!("\n========================================");
        println!("Warning: Some model files are missing:");
        println!("========================================\n");

        // 按模型分组显示
        let mut current_model: &str = "";
        for (model_name, file_name, _url) in &missing {
            if *model_name != current_model {
                if !current_model.is_empty() {
                    println!();
                }
                current_model = model_name;
                println!("[{}]", model_name);
            }
            println!("  - {}", file_name);
        }

        println!("\n----------------------------------------");
        println!("To download all missing models, run:");
        println!("  MODEL_AUTO_DOWNLOAD=1 cargo build");
        println!("\nOr manually download from:");
        for (_, _, url) in &missing {
            println!("  {}", url);
        }
        println!("========================================\n");
    }

    // 告诉 Cargo 如果 assets 目录改变则重新运行
    println!("cargo:rerun-if-changed=assets/");
}

/// 下载所有模型
fn download_all_models(project_root: &Path) {
    let models = get_models();

    println!("\n========================================");
    println!("Downloading model files...");
    println!("========================================\n");

    for model in &models {
        println!("\n[{}]", model.name);

        let model_dir = project_root.join(model.dir);

        // 创建目录
        if !model_dir.exists() {
            fs::create_dir_all(&model_dir)
                .unwrap_or_else(|_| panic!("Failed to create directory: {}", model_dir.display()));
        }

        for file in model.files {
            let file_path = model_dir.join(file.name);

            if file_path.exists() {
                println!("  {} already exists, skipping", file.name);
                continue;
            }

            println!("  Downloading {}...", file.name);

            // 使用 curl 下载
            let result = download_file(file.url, &file_path);

            match result {
                Ok(_) => println!("  ✓ Downloaded {}", file.name),
                Err(e) => {
                    eprintln!("  ✗ Failed to download {}: {}", file.name, e);
                    // 删除不完整的文件
                    let _ = fs::remove_file(&file_path);
                }
            }
        }
    }

    println!("\n========================================");
    println!("Model download complete!");
    println!("========================================\n");

    // 告诉 Cargo 如果 assets 目录改变则重新运行
    println!("cargo:rerun-if-changed=assets/");
}

/// 使用 curl 下载文件
fn download_file(url: &str, path: &Path) -> Result<(), String> {
    // 使用 curl 下载，-L 跟随重定向，-f 失败时无输出，-o 指定输出文件
    let output = Command::new("curl")
        .args(["-L", "-f", "-o"])
        .arg(path)
        .arg(url)
        .output()
        .map_err(|e| format!("Failed to run curl: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("Download failed: {}", stderr))
    }
}
