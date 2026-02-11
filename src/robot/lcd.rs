//! LCD 显示模块
//!
//! 240x240 RGB LCD 显示控制
//!
//! 使用 [electron_bot::ImageBuffer] 实现底层图片操作

use anyhow::Result;
use electron_bot::ImageBuffer;

// ==================== 常量 ====================

pub const LCD_WIDTH: usize = 240;
pub const LCD_HEIGHT: usize = 240;
pub const FRAME_SIZE: usize = LCD_WIDTH * LCD_HEIGHT * 3;

// ==================== DisplayMode ====================

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum DisplayMode {
    #[default]
    Static,
    Eyes,
    TestPattern,
}

// ==================== Lcd ====================

#[derive(Debug)]
pub struct Lcd {
    buffer: ImageBuffer,
    mode: DisplayMode,
    image_data: Option<Vec<u8>>,
}

impl Lcd {
    pub fn new() -> Self {
        Self {
            buffer: ImageBuffer::new(),
            mode: DisplayMode::default(),
            image_data: None,
        }
    }

    pub fn generate_pixels(&mut self) {
        match self.mode {
            DisplayMode::Static => self.render_static_image(),
            DisplayMode::Eyes => self.render_eyes(),
            DisplayMode::TestPattern => self.render_test_pattern(),
        }
    }

    /// 获取帧数据向量
    pub fn frame_vec(&mut self) -> Vec<u8> {
        self.generate_pixels();
        self.buffer.as_data().to_vec()
    }

    pub fn set_mode(&mut self, mode: DisplayMode) {
        self.mode = mode;
    }

    pub fn load_image(&mut self, path: &str) -> Result<()> {
        self.buffer
            .load_from_file(path)
            .map_err(|e| anyhow::anyhow!("Failed to load image {}: {}", path, e))?;
        self.image_data = Some(self.buffer.as_data().to_vec());
        Ok(())
    }

    fn render_static_image(&mut self) {
        if let Some(ref img) = self.image_data {
            if img.len() == FRAME_SIZE {
                self.buffer.as_mut_data().copy_from_slice(img);
            }
        } else {
            log::info!("Failed to load static image, show eyes");
            self.render_eyes();
        }
    }

    fn render_eyes(&mut self) {
        // 清空缓冲区
        self.buffer.clear(electron_bot::Color::Black);

        // 左眼
        self.buffer
            .fill_rect(40, 80, 80, 40, electron_bot::Color::White);
        // 右眼
        self.buffer
            .fill_rect(120, 80, 80, 40, electron_bot::Color::White);
    }

    fn render_test_pattern(&mut self) {
        // 简单的颜色条测试图案
        let colors = [
            electron_bot::Color::Red,
            electron_bot::Color::Green,
            electron_bot::Color::Blue,
            electron_bot::Color::Cyan,
            electron_bot::Color::Magenta,
            electron_bot::Color::Yellow,
        ];

        let block_height = LCD_HEIGHT / colors.len();
        for (i, color) in colors.iter().enumerate() {
            let y = i * block_height;
            self.buffer.fill_rect(0, y, LCD_WIDTH, block_height, *color);
        }
    }
}

impl Default for Lcd {
    fn default() -> Self {
        Self::new()
    }
}
