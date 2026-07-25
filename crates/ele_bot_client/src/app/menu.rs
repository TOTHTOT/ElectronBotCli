/// 菜单项
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuItem {
    DeviceStatus,
    DeviceControl,
    LlmTest,
    TtsTest,
    Settings,
    About,
}

impl MenuItem {
    #[must_use] 
    pub fn title(&self) -> &'static str {
        match self {
            MenuItem::DeviceStatus => "设备状态",
            MenuItem::DeviceControl => "设备控制",
            MenuItem::LlmTest => "LLM测试",
            MenuItem::TtsTest => "TTS测试",
            MenuItem::Settings => "设置",
            MenuItem::About => "关于",
        }
    }

    #[must_use] 
    pub fn all() -> [Self; 6] {
        [
            MenuItem::DeviceStatus,
            MenuItem::DeviceControl,
            MenuItem::LlmTest,
            MenuItem::TtsTest,
            MenuItem::Settings,
            MenuItem::About,
        ]
    }
}
