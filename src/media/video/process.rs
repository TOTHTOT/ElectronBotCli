//! 视频模块 - 图像处理

/// RGB 转换为 BGR
#[inline]
pub fn rgb_to_bgr(rgb_data: &[u8], _width: u32, _height: u32) -> Vec<u8> {
    let mut bgr_data = Vec::with_capacity(rgb_data.len());
    for chunk in rgb_data.chunks_exact(3) {
        bgr_data.extend_from_slice(&[chunk[2], chunk[1], chunk[0]]);
    }
    bgr_data
}

/// 处理视频帧（如添加人脸框）
pub fn process_frame(bgr_data: Vec<u8>, _width: u32, _height: u32) -> Vec<u8> {
    // TODO: 添加人脸检测功能
    bgr_data
}
