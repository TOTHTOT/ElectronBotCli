//! 测试模式模块 - 通过环境变量触发各种测试功能

/// 运行测试模式
pub fn run_test_mode() -> anyhow::Result<bool> {
    // 测试 RKNN 人脸检测 (RetinaFace)
    if std::env::var("TEST_RKNN").is_ok() {
        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        {
            use crate::model_manager::ModelManager;
            let mm = ModelManager::init()?;
            let Some(model_file) = mm.get("retinaface_rknn") else {
                anyhow::bail!("❌ 无法在缓存中找到 retinaface_rknn 模型文件");
            };

            let test_image = model_file.with_file_name("test.jpg");

            if !model_file.exists() {
                anyhow::bail!("❌ 模型文件不存在: {:?}", model_file);
            }

            log::info!("model_path: {:?}, test_image: {:?}", model_file, test_image);
            crate::vision::face::rknn::test_retinaface(model_file.clone(), test_image)?;
            log::info!("RetinaFace RKNN detection test finished!");
            return Ok(true);
        }
    }
    Ok(false)
}
