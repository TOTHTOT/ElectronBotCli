//! 视频模块 - 图像编码

use image::ImageEncoder;
use std::io::Cursor;

/// 将 BGR 数据编码为 JPEG
pub fn bgr_to_jpeg(bgr_data: &[u8], width: u32, height: u32, quality: u8) -> Option<Vec<u8>> {
    // BGR -> RGB
    let mut rgb_data = Vec::with_capacity((width * height * 3) as usize);
    for chunk in bgr_data.chunks(3) {
        if chunk.len() == 3 {
            rgb_data.push(chunk[2]); // R (from B)
            rgb_data.push(chunk[1]); // G
            rgb_data.push(chunk[0]); // B (from R)
        }
    }

    let mut jpeg_data = Vec::new();
    let mut cursor = Cursor::new(&mut jpeg_data);

    // 使用 image crate 编码 JPEG
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, quality);
    if let Err(e) = encoder.write_image(&rgb_data, width, height, image::ExtendedColorType::Rgb8) {
        log::error!("JPEG encoding failed: {:?}", e);
        return None;
    }

    Some(jpeg_data)
}

/// 将 RGB 数据编码为 JPEG
// pub fn rgb_to_jpeg(rgb_data: &[u8], width: u32, height: u32, quality: u8) -> Option<Vec<u8>> {
//     let mut jpeg_data = Vec::new();
//     let mut cursor = Cursor::new(&mut jpeg_data);
//
//     let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut cursor, quality);
//     if let Err(e) = encoder.write_image(rgb_data, width, height, image::ExtendedColorType::Rgb8) {
//         log::error!("JPEG encoding failed: {:?}", e);
//         return None;
//     }
//
//     Some(jpeg_data)
// }

/// 将 MJPEG 数据直接返回（已经是编码格式）
#[allow(dead_code)]
pub fn mjpeg_passthrough(mjpeg_data: Vec<u8>) -> Vec<u8> {
    mjpeg_data
}
