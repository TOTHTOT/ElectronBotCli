//! 事件模块 - 按功能分类的事件定义和处理

mod device;
mod menu;
mod settings;

pub use device::DeviceEvent;
pub use menu::MenuEvent;
pub use settings::SettingsEvent;

use crate::app::App;

/// 通用事件
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommonEvent {
    Quit,
    None,
}

/// 应用事件
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppEvent {
    Common(CommonEvent),
    Menu(MenuEvent),
    Device(DeviceEvent),
    Settings(SettingsEvent),
}

impl From<CommonEvent> for AppEvent {
    fn from(e: CommonEvent) -> Self {
        AppEvent::Common(e)
    }
}

impl From<MenuEvent> for AppEvent {
    fn from(e: MenuEvent) -> Self {
        AppEvent::Menu(e)
    }
}

impl From<DeviceEvent> for AppEvent {
    fn from(e: DeviceEvent) -> Self {
        AppEvent::Device(e)
    }
}

impl From<SettingsEvent> for AppEvent {
    fn from(e: SettingsEvent) -> Self {
        AppEvent::Settings(e)
    }
}

/// 处理应用事件
pub fn handle_event(app: &mut App, event: AppEvent) {
    match event {
        AppEvent::Common(CommonEvent::Quit) => {
            app.quit();
        }
        AppEvent::Common(CommonEvent::None) => {}
        AppEvent::Menu(e) => menu::handle(app, e),
        AppEvent::Device(e) => device::handle(app, e),
        AppEvent::Settings(e) => settings::handle(app, e),
    }
}
