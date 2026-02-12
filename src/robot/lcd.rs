//! LCD 显示模块
//!
//! 240x240 RGB LCD 显示控制
//!
//! 使用 [electron_bot::ImageBuffer] 实现底层图片操作
//! 使用 [boteyes] 库渲染机器人眼睛动画

use anyhow::Result;
use boteyes::{Mood, Position, RoboEyes};
use electron_bot::ImageBuffer;
use image::GrayImage;
// ==================== 常量 ====================

pub const LCD_WIDTH: usize = 240;
pub const LCD_HEIGHT: usize = 240;
pub const FRAME_SIZE: usize = LCD_WIDTH * LCD_HEIGHT * 3;

/// 计算数据的 FNV-1a 哈希值（用于检测内容变化）
fn compute_hash(data: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

// ==================== DisplayMode ====================

#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[allow(dead_code)]
pub enum DisplayMode {
    Static,
    #[default]
    Eyes,
    TestPattern,
}

// ==================== Lcd ====================

pub struct Lcd {
    buffer: ImageBuffer,
    mode: DisplayMode,
    image_data: Option<Vec<u8>>,
    eyes: RoboEyes,
    eyes_timer: u64,
    last_eyes_hash: Option<u64>, // 缓存上一帧的哈希值
}

#[allow(dead_code)]
impl Lcd {
    pub fn new() -> Self {
        let mut eyes = RoboEyes::new(240, 240);
        let mut buffer = GrayImage::new(240, 240);
        eyes.set_mood(Mood::Default);
        eyes.set_position(Position::Center);
        eyes.set_autoblinker(true, 3, 2);
        eyes.set_idle_mode(true, 2, 2);
        eyes.open();
        for i in 0..20 {
            eyes.draw_into(&mut buffer, i as u64 * 20);
        }
        eyes.set_mood(Mood::Default);
        eyes.draw_into(&mut buffer, 1000);

        Self {
            buffer: ImageBuffer::new(),
            mode: DisplayMode::default(),
            image_data: None,
            eyes,
            eyes_timer: 0,
            last_eyes_hash: None,
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
        let mut gray_buffer = GrayImage::new(LCD_WIDTH as u32, LCD_HEIGHT as u32);
        self.eyes.draw_into(&mut gray_buffer, self.eyes_timer);
        self.eyes_timer = self.eyes_timer.wrapping_add(50);

        let current_hash = compute_hash(gray_buffer.as_raw());
        if Some(current_hash) != self.last_eyes_hash {
            self.last_eyes_hash = Some(current_hash);
            for (i, pixel) in gray_buffer.pixels().enumerate() {
                let gray = pixel.0[0];
                let rgb_idx = i * 3;
                self.buffer.as_mut_data()[rgb_idx] = gray; // R
                self.buffer.as_mut_data()[rgb_idx + 1] = gray; // G
                self.buffer.as_mut_data()[rgb_idx + 2] = gray; // B
            }
        }
    }

    /// 设置眼睛表情
    pub fn set_eyes_mood(&mut self, mood: boteyes::Mood) {
        self.eyes.set_mood(mood);
    }

    /// 设置眼睛注视方向
    pub fn set_eyes_position(&mut self, position: boteyes::Position) {
        self.eyes.set_position(position);
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
