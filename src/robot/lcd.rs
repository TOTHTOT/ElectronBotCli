//! LCD 显示模块
//!
//! 240x240 RGB LCD 显示控制
//!
//! 使用 [electron_bot::ImageBuffer] 实现底层图片操作
//! 使用 [boteyes] 库渲染机器人眼睛动画

use anyhow::Result;
use boteyes::RoboEyes;
use electron_bot::ImageBuffer;

// ==================== 常量 ====================

pub const LCD_WIDTH: usize = 240;
pub const LCD_HEIGHT: usize = 240;
pub const FRAME_SIZE: usize = LCD_WIDTH * LCD_HEIGHT * 3;

// 眼睛画布大小 (BotEyes 默认输出)
const EYE_CANVAS_WIDTH: usize = 128;
const EYE_CANVAS_HEIGHT: usize = 128;

// 导出 BotEyes 类型

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
}

#[allow(dead_code)]
impl Lcd {
    pub fn new() -> Self {
        let mut eyes = RoboEyes::new(128, 168);
        eyes.set_mood(boteyes::Mood::Default);
        eyes.open();
        eyes.set_autoblinker(true, 4000, 2000);
        eyes.set_size(EYE_CANVAS_WIDTH as u32, EYE_CANVAS_HEIGHT as u32); // 眼睛大小

        Self {
            buffer: ImageBuffer::new(),
            mode: DisplayMode::default(),
            image_data: None,
            eyes,
            eyes_timer: 0,
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
        // 使用 BotEyes 库渲染眼睛动画 (返回 128x64 灰度图)
        let eye_img = self.eyes.draw_eyes(self.eyes_timer);
        self.eyes_timer += 16; // ~60fps

        // 清空缓冲区
        self.buffer.clear(electron_bot::Color::Black);

        // 居中显示在 LCD 上
        let eye_offset_x =(LCD_WIDTH - EYE_CANVAS_WIDTH) / 2 -20;
        let eye_offset_y = (LCD_HEIGHT - EYE_CANVAS_HEIGHT) / 2 -20;
        // let eye_offset_x = 0;
        // let eye_offset_y = 0;
        // 遍历整个眼睛画布，复制非黑色像素
        for y in 0..EYE_CANVAS_HEIGHT {
            for x in 0..EYE_CANVAS_WIDTH {
                let pixel = eye_img.get_pixel(x as u32, y as u32);
                let gray = pixel[0];

                if gray > 0 {
                    let screen_x = eye_offset_x + x;
                    let screen_y = eye_offset_y + y;
                    self.buffer.set_pixel(screen_x, screen_y, electron_bot::Color::Red);
                }
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
