//! 视频模块 - 图像编码

/// MJPEG 直通（无需处理）
#[allow(dead_code)]
#[must_use] 
pub fn mjpeg_passthrough(mjpeg_data: Vec<u8>) -> Vec<u8> {
    mjpeg_data
}
