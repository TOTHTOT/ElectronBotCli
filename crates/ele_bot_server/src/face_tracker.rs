//! 人脸追踪模块
//!
//! 根据人脸位置计算机器人身体(舵机 5)需要调整的角度, 实现自动居中控制。

/// 根据人脸位置计算机器人身体需要调整的角度
///
/// # Arguments
///
/// * `face_x` - 人脸归一化 x 坐标 (0-1)
///
/// # Returns
///
/// 身体转动角度调整值 (度)
#[must_use] 
pub fn calculate_body_adjustment(face_x: f32) -> i32 {
    // 人脸在画面中心(0.5)时不需要调整
    // 每偏移 0.1 移动 5 度身体
    ((face_x - 0.5) * 50.0).round() as i32
}

/// 平滑处理角度调整值
#[must_use] 
pub fn smooth_adjustment(current: i32, target: i32, smoothing: f32) -> i32 {
    (current as f32 + (target as f32 - current as f32) * smoothing).round() as i32
}

/// 身体对应的舵机索引
pub const BODY_SERVO_INDEX: usize = 5;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_returns_zero() {
        assert_eq!(calculate_body_adjustment(0.5), 0);
    }

    #[test]
    fn right_offset_positive() {
        assert!(calculate_body_adjustment(0.7) > 0);
    }

    #[test]
    fn left_offset_negative() {
        assert!(calculate_body_adjustment(0.3) < 0);
    }

    #[test]
    fn smooth_no_change() {
        assert_eq!(smooth_adjustment(10, 10, 0.3), 10);
    }

    #[test]
    fn smooth_full() {
        assert_eq!(smooth_adjustment(0, 10, 1.0), 10);
    }

    #[test]
    fn smooth_zero_keeps_current() {
        assert_eq!(smooth_adjustment(10, 20, 0.0), 10);
    }
}
