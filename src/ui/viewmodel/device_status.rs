use crate::app::App;

pub struct DeviceStatusViewModel {
    pub is_connected: bool,
    pub battery: u32,
    pub network: &'static str,
    pub volume: i32,
}

impl DeviceStatusViewModel {
    pub fn from_app(app: &App) -> Self {
        Self {
            is_connected: app.is_connected(),
            battery: 85, // TODO: 后续获取真实电量
            network: "已连接",
            volume: app.ai.voice_manager.volume(),
        }
    }
}
