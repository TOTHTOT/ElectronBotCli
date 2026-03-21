//! 事件模块 - 按功能分类的事件定义和处理

mod device;
mod llm_test;
mod menu;
mod settings;

pub use device::DeviceEvent;
pub use llm_test::LlmTestEvent;
pub use menu::MenuEvent;
pub use settings::SettingsEvent;

use crate::app::{App, MenuItem};
use crate::input::llm_test::handle;
use crossterm::event::{KeyCode, KeyModifiers};

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
    LlmTest(LlmTestEvent),
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

impl From<LlmTestEvent> for AppEvent {
    fn from(e: LlmTestEvent) -> Self {
        AppEvent::LlmTest(e)
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
        AppEvent::LlmTest(_) => {} // Handled in handle_by_mode
    }
}

/// 根据应用模式分发输入处理
///
/// 根据弹窗状态和三个模式标志位（编辑/伺服/设置）确定当前模式，
/// 然后调用对应的模式处理函数
///
/// # Arguments
///
/// * `app` - 应用状态
/// * `code` - 按键代码
/// * `modifiers` - 修饰键状态
pub fn handle_by_mode(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    // 弹窗模式具有最高优先级
    if app.popup.is_visible() {
        handle_popup_mode(app, code);
        return;
    }

    // 使用模式元组进行模式匹配
    match (
        app.ui.in_edit_settings_mode,
        app.in_servo_mode,
        app.ui.in_settings,
        app.ai.in_llm_test_mode,
    ) {
        // 编辑模式：处理设置项内容编辑
        (true, _, _, _) => handle_edit_settings_mode(app, code),
        // 设备控制模式：处理舵机角度调整
        (_, true, _, _) => handle_servo_mode(app, code),
        // 设置模式：处理配置项选择
        (_, _, true, _) => handle_settings_mode(app, code),
        // LLM 测试模式：处理文本输入
        (_, _, _, true) => handle_llm_test_mode(app, code),
        // 菜单模式：处理侧边栏导航
        _ => handle_menu_mode(app, code, modifiers),
    }
}

/// 菜单模式输入处理
///
/// 处理侧边栏导航相关的按键输入：
/// - 上/下方向键：切换菜单项
/// - 回车键：进入对应功能页面
/// - ESC 或 Ctrl+C：退出程序
/// - Ctrl+S：保存设置
///
/// # Arguments
///
/// * `app` - 应用状态
/// * `code` - 按键代码
/// * `modifiers` - 修饰键状态
fn handle_menu_mode(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    let evt = match code {
        KeyCode::Esc => CommonEvent::Quit.into(),
        KeyCode::Up => MenuEvent::Up.into(),
        KeyCode::Down => MenuEvent::Down.into(),
        KeyCode::Enter => handle_menu_enter(app),
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => CommonEvent::Quit.into(),
        KeyCode::Char('s') if modifiers.contains(KeyModifiers::CONTROL) => {
            SettingsEvent::Save.into()
        }
        _ => CommonEvent::None.into(),
    };
    handle_event(app, evt);
}

/// 处理菜单模式下的回车键
///
/// 根据当前选中的菜单项触发相应的事件：
/// - 设备控制：进入伺服模式
/// - 设置：进入设置模式
/// - LLM测试：进入LLM测试模式
/// - 其他：尝试连接设备
///
/// # Arguments
///
/// * `app` - 应用状态
///
/// # Returns
///
/// 对应的事件
fn handle_menu_enter(app: &mut App) -> AppEvent {
    match app.ui.selected_menu {
        MenuItem::DeviceControl => MenuEvent::EnterServoMode.into(),
        MenuItem::Settings => MenuEvent::EnterSettingMode.into(),
        MenuItem::LlmTest => {
            app.ai.in_llm_test_mode = true;
            app.ui.left_focused = false;
            AppEvent::LlmTest(LlmTestEvent::None)
        }
        _ => MenuEvent::ConnectDevice.into(),
    }
}

/// 设备控制模式输入处理
///
/// 处理舵机控制界面的按键输入：
/// - 焦点在左侧时：退出伺服模式
/// - 上/下方向键：切换选中关节
/// - 左/右方向键：减小/增大关节角度
/// - S键：截图保存
/// - ESC/回车键：退出伺服模式
///
/// # Arguments
///
/// * `app` - 应用状态
/// * `code` - 按键代码
fn handle_servo_mode(app: &mut App, code: KeyCode) {
    if app.ui.left_focused {
        app.in_servo_mode = false;
        return;
    }

    match code {
        KeyCode::Esc => {
            app.toggle_focus();
            app.in_servo_mode = false;
        }
        KeyCode::Enter => {
            app.toggle_focus();
            app.in_servo_mode = false;
        }
        KeyCode::Up => handle_event(app, DeviceEvent::Prev.into()),
        KeyCode::Down => handle_event(app, DeviceEvent::Next.into()),
        KeyCode::Left => handle_event(app, DeviceEvent::Decrease.into()),
        KeyCode::Right => handle_event(app, DeviceEvent::Increase.into()),
        KeyCode::Char('s') => handle_event(app, DeviceEvent::Screenshot.into()),
        _ => {}
    }
}

/// 设置模式输入处理
///
/// 处理设置界面的按键输入：
/// - 焦点在左侧时：退出设置模式
/// - 上/下方向键：切换设置项
/// - 回车键：进入编辑模式
/// - ESC键：退出设置模式
///
/// # Arguments
///
/// * `app` - 应用状态
/// * `code` - 按键代码
fn handle_settings_mode(app: &mut App, code: KeyCode) {
    if app.ui.left_focused {
        app.ui.in_settings = false;
        return;
    }

    let evt = match code {
        KeyCode::Esc => {
            app.toggle_focus();
            app.ui.in_settings = false;
            SettingsEvent::Exit.into()
        }
        KeyCode::Enter => SettingsEvent::EnterEdit.into(),
        KeyCode::Up => SettingsEvent::Up.into(),
        KeyCode::Down => SettingsEvent::Down.into(),
        _ => CommonEvent::None.into(),
    };
    handle_event(app, evt);
}

/// 编辑模式输入处理
///
/// 处理设置项内容编辑的按键输入：
/// - ESC键：取消编辑，丢弃修改
/// - 回车键：确认保存修改
/// - 退格键：删除最后一个字符
/// - 普通字符：追加到编辑缓冲区
///
/// # Arguments
///
/// * `app` - 应用状态
/// * `code` - 按键代码
fn handle_edit_settings_mode(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Esc => app.cancel_settings_edit(),
        KeyCode::Enter => app.save_settings_edit(),
        KeyCode::Backspace => {
            app.ui.edit_buffer.pop();
        }
        KeyCode::Char(c) => {
            app.ui.edit_buffer.push(c);
        }
        _ => {}
    }
}

/// 弹窗模式输入处理
///
/// 处理模态弹窗的按键输入，目前仅响应ESC键关闭弹窗
///
/// # Arguments
///
/// * `app` - 应用状态
/// * `code` - 按键代码
fn handle_popup_mode(app: &mut App, code: KeyCode) {
    if matches!(code, KeyCode::Esc) {
        app.stop_comm_thread();
    }
}

fn handle_llm_test_mode(app: &mut App, code: KeyCode) {
    if app.ui.left_focused {
        if code == KeyCode::Tab
            || !matches!(
                code,
                KeyCode::Esc | KeyCode::Enter | KeyCode::Up | KeyCode::Down
            )
        {
            app.toggle_focus();
        }
    } else if code == KeyCode::Esc || code == KeyCode::Tab {
        app.ai.in_llm_test_mode = false;
        app.ai.llm_test_state.input_text.clear();
        app.ai.llm_test_state.output_text.clear();
        app.ai.llm_test_state.current_mood = None;
        app.toggle_focus();
    } else {
        handle(app, code);
    }
}
