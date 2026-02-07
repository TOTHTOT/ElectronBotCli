use crate::app::eyes::RobotEyes;

/// 显示模式
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DisplayMode {
    RobotEyes,
    Image,
}

/// 屏幕显示控制器
pub struct DisplayController {
    mode: DisplayMode,
    // 这个用于保存图片数据, 虽然也很大, 但是和 App::shared_state::pixels 不是同一个东西,
    // 实际渲染会被复制到pixels, 这样做免得频繁读文件
    image_data: Option<Vec<u8>>,
}

impl Default for DisplayController {
    fn default() -> Self {
        Self {
            mode: DisplayMode::RobotEyes,
            image_data: None,
        }
    }
}

impl DisplayController {
    pub fn new() -> Self {
        Self::default()
    }

    /// 生成像素数据 (240x240)
    pub fn generate_pixels(&mut self, pixels: &mut [u8]) {
        match self.mode {
            DisplayMode::RobotEyes => self.generate_eyes_pixels(pixels),
            DisplayMode::Image => self.generate_image_pixels(pixels),
        }
    }

    /// 生成机器眼睛像素
    fn generate_eyes_pixels(&self, pixels: &mut [u8]) {
        let mut eyes = RobotEyes::new();
        eyes.random_blink();
        eyes.generate_frame(pixels);
    }

    /// 生成图片像素
    fn generate_image_pixels(&self, pixels: &mut [u8]) {
        if let Some(ref image_data) = self.image_data {
            pixels.copy_from_slice(image_data);
        }
    }

    /// 设置显示模式
    pub fn set_mode(&mut self, mode: DisplayMode) {
        self.mode = mode;
    }

    /// 获取当前显示模式
    pub fn get_mode(&self) -> DisplayMode {
        self.mode
    }

    /// 设置静态图片
    pub fn set_image(&mut self, image_data: &[u8]) {
        self.image_data = Some(image_data.to_vec());
        self.mode = DisplayMode::Image;
    }
}
