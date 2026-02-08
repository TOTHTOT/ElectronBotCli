//! LCD 显示模块
//!
//! 240x240 RGB LCD 显示控制

use anyhow::{Context, Result};
use image::imageops::FilterType;

// ==================== 常量 ====================

pub const LCD_WIDTH: usize = 240;
pub const LCD_HEIGHT: usize = 240;
pub const FRAME_SIZE: usize = LCD_WIDTH * LCD_HEIGHT * 3;
pub const BUFFER_COUNT: usize = 2;

// ==================== ImageProcessor ====================

#[derive(Debug, Default)]
pub struct ImageProcessor;

impl ImageProcessor {
    pub fn new() -> Self {
        Self
    }

    pub fn load_from_file(&mut self, path: &str) -> Result<Vec<u8>> {
        let img = image::open(path).context(format!("Could not load image {path}"))?;
        let resized = img.resize_exact(LCD_WIDTH as u32, LCD_HEIGHT as u32, FilterType::Triangle);
        let rgb_data = resized.to_rgb8();
        Ok(rgb_data.to_vec())
    }
}

// ==================== DisplayMode ====================

#[derive(Clone, Copy, Debug, Default)]
pub enum DisplayMode {
    #[default]
    Static,
    Eyes,
}

// ==================== Lcd ====================

#[derive(Debug, Default)]
pub struct Lcd {
    pixels: Vec<u8>,
    mode: DisplayMode,
    image_data: Option<Vec<u8>>,
}

impl Lcd {
    pub fn new() -> Self {
        Self {
            pixels: vec![0u8; FRAME_SIZE],
            mode: DisplayMode::default(),
            image_data: None,
        }
    }

    pub fn generate_pixels(&mut self) {
        match self.mode {
            DisplayMode::Static => self.render_static_image(),
            DisplayMode::Eyes => self.render_eyes(),
        }
    }

    pub fn frame_vec(&mut self) -> Vec<u8> {
        self.generate_pixels();
        self.pixels.clone()
    }

    pub fn set_mode(&mut self, mode: DisplayMode) {
        self.mode = mode;
    }

    pub fn load_image(&mut self, path: &str) -> Result<()> {
        let mut processor = ImageProcessor::new();
        self.image_data = Some(processor.load_from_file(path)?);
        Ok(())
    }

    fn render_static_image(&mut self) {
        if let Some(ref img) = self.image_data {
            if img.len() == FRAME_SIZE {
                self.pixels.copy_from_slice(img);
            }
        } else {
            self.render_eyes();
        }
    }

    fn render_eyes(&mut self) {
        // 绘制眼睛
        self.pixels.fill(0);

        // 左眼
        self.draw_rect(40, 80, 80, 40, [255, 255, 255]);
        // 右眼
        self.draw_rect(120, 80, 80, 40, [255, 255, 255]);
    }

    fn draw_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: [u8; 3]) {
        for dy in 0..h {
            for dx in 0..w {
                let px = x + dx;
                let py = y + dy;
                if px < LCD_WIDTH && py < LCD_HEIGHT {
                    let idx = (py * LCD_WIDTH + px) * 3;
                    self.pixels[idx] = color[0];
                    self.pixels[idx + 1] = color[1];
                    self.pixels[idx + 2] = color[2];
                }
            }
        }
    }
}
