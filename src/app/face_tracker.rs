//! 人脸追踪模块
//!
//! 根据人脸位置计算机器人需要调整的角度，实现自动居中控制

/// 根据人脸位置计算机器人需要调整的身体角度
///
/// # Arguments
///
/// * `face_x` - 人脸归一化 x 坐标 (0-1)
///
/// # Returns
///
/// 身体上下转动角度调整值 (度)
pub fn calculate_body_adjustment(face_x: f32) -> i32 {
    // x方向: 身体上下转动
    // 人脸在画面中心(0.5)时不需要调整
    // 每偏移0.1移动5度身体
    ((face_x - 0.5) * 50.0).round() as i32
}

/// 平滑处理角度调整值
///
/// # Arguments
///
/// * `current` - 当前调整值
/// * `target` - 目标调整值
/// * `smoothing` - 平滑系数 (0-1)，越小越平滑
///
/// # Returns
///
/// 平滑后的调整值
pub fn smooth_adjustment(current: i32, target: i32, smoothing: f32) -> i32 {
    (current as f32 + (target as f32 - current as f32) * smoothing).round() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_body_adjustment() {
        // 中心位置应该返回 0
        assert_eq!(calculate_body_adjustment(0.5), 0);

        // 向下偏移应该返回正值
        assert!(calculate_body_adjustment(0.7) > 0);

        // 向上偏移应该返回负值
        assert!(calculate_body_adjustment(0.3) < 0);
    }

    #[test]
    fn test_smooth_adjustment() {
        // 相同值应该返回相同值
        assert_eq!(smooth_adjustment(10, 10, 0.3), 10);

        // 平滑系数为1应该直接返回目标值
        assert_eq!(smooth_adjustment(0, 10, 1.0), 10);

        // 平滑系数为0应该保持原值
        assert_eq!(smooth_adjustment(10, 20, 0.0), 10);
    }
}
