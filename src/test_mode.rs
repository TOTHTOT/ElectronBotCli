//! 测试模式模块 - 通过环境变量触发各种测试功能

/// 运行测试模式
pub fn run_test_mode() -> anyhow::Result<bool> {
    // 测试 RKNN 人脸检测 (RetinaFace)
    if std::env::var("TEST_RKNN").is_ok() {
        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        {
            use crate::model_manager::ModelManager;
            let mm = ModelManager::init()?;
            // 1. 获取模型全路径 (例如: .../snapshots/xxx/model/RetinaFace.rknn)
            let Some(model_file) = mm.get("retinaface_rknn") else {
                anyhow::bail!("❌ 无法在缓存中找到 retinaface_rknn 模型文件");
            };

            // 2. 推导测试图片路径 (替换文件名即可)
            // with_file_name 会把 "RetinaFace.rknn" 替换成 "test.jpg"
            // 这样无论snapshots路径怎么变，都能精准找到同目录下的图
            let test_image = model_file.with_file_name("test.jpg");

            // 3. 最终验证与运行
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
