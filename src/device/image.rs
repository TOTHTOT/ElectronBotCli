use image::{DynamicImage, GenericImageView, ImageBuffer, Pixel};
use std::error::Error;

pub const FRAME_WIDTH: usize = 240;
pub const FRAME_HEIGHT: usize = 240;
pub const IMAGE_FRAME_SIZE: usize = FRAME_WIDTH * FRAME_HEIGHT * 3; // 172800 bytes

/// RGB888 图像处理器
pub struct ImageProcessor {
    frame_buffer: [u8; IMAGE_FRAME_SIZE],
}

impl ImageProcessor {
    pub fn new() -> Self {
        Self {
            frame_buffer: [0u8; IMAGE_FRAME_SIZE],
        }
    }

    /// 从文件加载并处理图像
    pub fn load_from_file(&mut self, path: &str) -> Result<&[u8], Box<dyn Error>> {
        let img = image::open(path)?;
        self.process(img)
    }

    /// 从内存加载 BGRA 格式数据
    pub fn load_from_bgra(
        &mut self,
        data: &[u8],
        width: u32,
        height: u32,
    ) -> Result<&[u8], Box<dyn Error>> {
        // BGRA -> RGBA (交换 R 和 B)
        let mut rgba_data = data.to_vec();
        for chunk in rgba_data.chunks_mut(4) {
            chunk.swap(0, 2); // 交换 R 和 B
        }
        let img: ImageBuffer<image::Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_raw(width, height, rgba_data)
                .ok_or("Failed to create image buffer")?;
        let img = DynamicImage::ImageRgba8(img);
        self.process(img)
    }

    /// 从内存加载 RGBA 格式数据
    pub fn load_from_rgba(
        &mut self,
        data: &[u8],
        width: u32,
        height: u32,
    ) -> Result<&[u8], Box<dyn Error>> {
        let img: ImageBuffer<image::Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_raw(width, height, data.to_vec())
                .ok_or("Failed to create image buffer")?;
        let img = DynamicImage::ImageRgba8(img);
        self.process(img)
    }

    /// 内部处理流程
    fn process(&mut self, img: DynamicImage) -> Result<&[u8], Box<dyn Error>> {
        let img = img.flipv();

        let img = img.resize(
            FRAME_WIDTH as u32,
            FRAME_HEIGHT as u32,
            image::imageops::FilterType::Nearest,
        );

        let (w, h) = img.dimensions();
        let mut idx = 0;

        for _y in 0..h {
            for _x in 0..w {
                let pixel = img.get_pixel(_x, _y);
                let channels = pixel.channels();

                let (r, g, b) = match channels.len() {
                    4 => (channels[0], channels[1], channels[2]), // RGBA -> RGB
                    3 => (channels[0], channels[1], channels[2]), // RGB -> RGB
                    _ => (0, 0, 0),
                };

                self.frame_buffer[idx] = r;
                self.frame_buffer[idx + 1] = g;
                self.frame_buffer[idx + 2] = b;
                idx += 3;
            }
        }

        Ok(&self.frame_buffer)
    }

    /// 获取帧缓冲区指针
    pub fn frame_data(&self) -> &[u8; IMAGE_FRAME_SIZE] {
        &self.frame_buffer
    }

    /// 作为 Vec 返回
    pub fn frame_data_vec(&self) -> Vec<u8> {
        self.frame_buffer.to_vec()
    }
}

impl Default for ImageProcessor {
    fn default() -> Self {
        Self::new()
    }
}
