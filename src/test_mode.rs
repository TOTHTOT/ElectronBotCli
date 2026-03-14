//! 测试模式模块 - 通过环境变量触发各种测试功能

/// 运行测试模式
pub fn run_test_mode() -> anyhow::Result<bool> {
    // 测试 RKNN 人脸检测
    if std::env::var("TEST_RKNN").is_ok() {
        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        {
            log::info!("Running RKNN face detection test...");
            let model_path = std::env::var("RKNN_MODEL")
                .unwrap_or_else(|_| "./model/deepghs/yolo-face/yolo_face.rknn".to_string());
            let test_image = std::env::var("TEST_IMAGE")
                .unwrap_or_else(|_| "./assets/images/figure1.png".to_string());
            crate::vision::face::rknn::test_face_detection(&model_path, &test_image)?;
            log::info!("RKNN face detection test finished!");
            return Ok(true);
        }
    }
    Ok(false)
}
