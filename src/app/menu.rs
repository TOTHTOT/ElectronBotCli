/// 菜单项
#[derive(Clone, Copy, PartialEq)]
pub enum MenuItem {
    DeviceStatus,
    DeviceControl,
    Settings,
    About,
}

impl MenuItem {
    pub fn title(&self) -> &'static str {
        match self {
            MenuItem::DeviceStatus => "设备状态",
            MenuItem::DeviceControl => "设备控制",
            MenuItem::Settings => "设置",
            MenuItem::About => "关于",
        }
    }

    pub fn all() -> [Self; 4] {
        [
            MenuItem::DeviceStatus,
            MenuItem::DeviceControl,
            MenuItem::Settings,
            MenuItem::About,
        ]
    }
}
