use crate::app::constants::{FRAME_HEIGHT, FRAME_WIDTH};

/// 眼睛颜色
#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(dead_code)]
pub enum EyeColor {
    White,  // 白色眼睛
    Blue,   // 蓝色
    Green,  // 绿色
    Red,    // 红色
    Yellow, // 黄色
}

/// 表情类型
#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(dead_code)]
pub enum Expression {
    Neutral,   // 中性
    Happy,     // 开心
    Sad,       // 难过
    Angry,     // 生气
    Surprised, // 惊讶
    Wink,      // 眨眼
    Blink,     // 闭眼
    LookLeft,  // 看左
    LookRight, // 看右
    LookUp,    // 看上
    LookDown,  // 看下
}

/// 眼睛大小
#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(dead_code)]
pub enum EyeSize {
    Small,
    Medium,
    Large,
}

/// 机器人眼睛显示控制器 (适配 240x240 LCD)
pub struct RobotEyes {
    expression: Expression,
    eye_color: EyeColor,
    eye_size: EyeSize,
    blink_timer: u32,
    is_blinking: bool,
    blink_progress: u32,
    frame_counter: u32,
}

impl Default for RobotEyes {
    fn default() -> Self {
        Self {
            expression: Expression::Neutral,
            eye_color: EyeColor::White,
            eye_size: EyeSize::Medium,
            blink_timer: 0,
            is_blinking: false,
            blink_progress: 0,
            frame_counter: 0,
        }
    }
}

#[allow(dead_code)]
impl RobotEyes {
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置表情
    pub fn set_expression(&mut self, expression: Expression) {
        self.expression = expression;
    }

    /// 获取当前表情
    pub fn get_expression(&self) -> Expression {
        self.expression
    }

    /// 设置眼睛颜色
    pub fn set_eye_color(&mut self, color: EyeColor) {
        self.eye_color = color;
    }

    /// 设置眼睛大小
    pub fn set_eye_size(&mut self, size: EyeSize) {
        self.eye_size = size;
    }

    /// 随机眨眼
    pub fn random_blink(&mut self) {
        if !self.is_blinking && self.blink_timer == 0 {
            self.is_blinking = true;
            self.blink_progress = 0;
            self.blink_timer = 120 + rand::random::<u32>() % 180; // 2-4秒后再次眨眼
        }
    }

    /// 是否正在眨眼
    pub fn is_blinking(&self) -> bool {
        self.is_blinking
    }

    /// 生成一帧 (240x240)
    pub fn generate_frame(&mut self, pixels: &mut [u8]) {
        self.frame_counter = self.frame_counter.wrapping_add(1);

        // 更新眨眼
        self.update_blink();

        // 填充黑色背景
        for byte in pixels.iter_mut() {
            *byte = 0;
        }

        // 根据眼睛大小确定位置 (适配 240 行高度)
        let (eye_width, eye_height, eye_y) = match self.eye_size {
            EyeSize::Small => (60, 45, 70),
            EyeSize::Medium => (80, 60, 60),
            EyeSize::Large => (100, 75, 50),
        };

        let eye_gap = 20;
        let left_eye_x = (FRAME_WIDTH as i32 - eye_gap - eye_width as i32) / 2;
        let right_eye_x = left_eye_x + eye_gap + eye_width as i32;

        // 绘制眼睛
        match self.expression {
            Expression::Blink | Expression::Wink => {
                self.draw_eye_line(pixels, left_eye_x, eye_y, eye_width, eye_height);
                self.draw_eye_line(pixels, right_eye_x, eye_y, eye_width, eye_height);
            }
            _ => {
                self.draw_eye(pixels, left_eye_x, eye_y, eye_width, eye_height);
                self.draw_eye(pixels, right_eye_x, eye_y, eye_width, eye_height);
            }
        }
    }

    fn update_blink(&mut self) {
        if self.is_blinking {
            self.blink_progress += 1;
            if self.blink_progress >= 10 {
                self.is_blinking = false;
                self.expression = Expression::Neutral;
            }
        } else if self.blink_timer > 0 {
            self.blink_timer -= 1;
        }
    }

    fn draw_eye(&self, pixels: &mut [u8], x: i32, y: i32, w: usize, h: usize) {
        let color = match self.eye_color {
            EyeColor::White => (255, 255, 255),
            EyeColor::Blue => (50, 100, 255),
            EyeColor::Green => (50, 255, 50),
            EyeColor::Red => (255, 50, 50),
            EyeColor::Yellow => (255, 255, 50),
        };

        // 椭圆眼睛
        for py in 0..h {
            for px in 0..w {
                let cx = w as f32 / 2.0;
                let cy = h as f32 / 2.0;
                let dx = (px as f32 - cx) / cx;
                let dy = (py as f32 - cy) / cy;
                if dx * dx + dy * dy <= 1.0 {
                    self.set_pixel(pixels, x + px as i32, y + py as i32, color);
                }
            }
        }

        // 眼珠 (瞳孔)
        let pupil_radius = (w / 8) as i32;
        let pupil_x = x + (w as i32 / 2);
        let pupil_y = y + (h as i32 / 2);
        self.draw_circle(pixels, pupil_x, pupil_y, pupil_radius, (0, 0, 0));
    }

    fn draw_eye_line(&self, pixels: &mut [u8], x: i32, y: i32, w: usize, h: usize) {
        let line_y = y + (h as i32 / 2);
        for px in 0..w {
            self.set_pixel(pixels, x + px as i32, line_y, (100, 100, 100));
        }
    }

    fn set_pixel(&self, pixels: &mut [u8], x: i32, y: i32, color: (u8, u8, u8)) {
        if x >= 0 && x < FRAME_WIDTH as i32 && y >= 0 && y < FRAME_HEIGHT as i32 {
            let idx = (y as usize * FRAME_WIDTH + x as usize) * 3;
            if idx + 2 < pixels.len() {
                pixels[idx] = color.0;
                pixels[idx + 1] = color.1;
                pixels[idx + 2] = color.2;
            }
        }
    }

    fn draw_circle(&self, pixels: &mut [u8], cx: i32, cy: i32, r: i32, color: (u8, u8, u8)) {
        let r_sq = r * r;
        for y in -r..=r {
            for x in -r..=r {
                if x * x + y * y <= r_sq {
                    self.set_pixel(pixels, cx + x, cy + y, color);
                }
            }
        }
    }
}

/// 简单的眼睛动画控制器
pub struct EyeController {
    eyes: RobotEyes,
}

impl Default for EyeController {
    fn default() -> Self {
        Self {
            eyes: RobotEyes::new(),
        }
    }
}

impl EyeController {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn generate_frame(&mut self, pixels: &mut [u8]) {
        self.eyes.random_blink();
        self.eyes.generate_frame(pixels);
    }

    pub fn set_expression(&mut self, expression: Expression) {
        self.eyes.set_expression(expression);
    }

    pub fn set_eye_color(&mut self, color: EyeColor) {
        self.eyes.set_eye_color(color);
    }

    pub fn set_eye_size(&mut self, size: EyeSize) {
        self.eyes.set_eye_size(size);
    }
}
