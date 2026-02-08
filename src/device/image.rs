use anyhow::{Context, Result};
use image::imageops::FilterType;

/// 图像处理器
pub struct ImageProcessor;

impl ImageProcessor {
    pub fn new() -> Self {
        Self
    }

    /// 从文件加载并处理图像，返回 RGB888 格式的 Vec<u8>
    pub fn load_from_file(&mut self, path: &str) -> Result<Vec<u8>> {
        let img = image::open(path).context(format!("Could not load image {path}"))?;
        let resized = img.resize_exact(240, 240, FilterType::Triangle);
        let rgb_data = resized.to_rgb8();
        Ok(rgb_data.to_vec())
    }
}

impl Default for ImageProcessor {
    fn default() -> Self {
        Self::new()
    }
}
