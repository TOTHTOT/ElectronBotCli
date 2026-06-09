use crate::app::App;

pub struct DeviceStatusViewModel {
    pub is_connected: bool,
    pub battery: u32,
    pub network: &'static str,
    pub volume: i32,
}

impl DeviceStatusViewModel {
    pub fn from_app(app: &App) -> Self {
        let server = app.server.lock().unwrap();
        Self {
            is_connected: server.robot_connected,
            battery: 85, // TODO: 后续获取真实电量
            network: if server.net_connected {
                "已连接"
            } else {
                "未连接"
            },
            volume: server.volume,
        }
    }
}
