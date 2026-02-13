extern crate log;

mod app;
mod input;
mod robot;
mod ui;
mod ui_components;
mod voice;

use crate::voice::VoiceManager;
use crossterm::event::KeyModifiers;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::prelude::*;
use simplelog::{CombinedLogger, Config, WriteLogger};
use std::fs::File;
use std::io::{self, Stdout};
use std::time::Duration;

fn main() -> anyhow::Result<()> {
    let log_file = File::create("ele_bot.log").ok();
    if let Some(f) = log_file {
        CombinedLogger::init(vec![WriteLogger::new(
            simplelog::LevelFilter::Trace,
            Config::default(),
            f,
        )])
        .ok();
    }
    let voice_manager = VoiceManager::new("assets/module/vosk-model-small-cn-0.22", "麦克风阵列")?;
    let mut stdout = io::stdout();
    enable_raw_mode()?;
    stdout.execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;
    run(&mut terminal, voice_manager)?;
    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;

    Ok(())
}

/// 渲染tui界面
///
/// # Arguments
///
/// * `terminal` - 终端
/// * `voice_manager` - 语音管理器
///
/// # Returns
///
/// Result<(), Error> - 错误信息
fn run(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    voice_manager: VoiceManager,
) -> anyhow::Result<()> {
    let mut app = app::App::new(voice_manager);
    let tick_rate = Duration::from_millis(20);
    while app.running {
        if app.is_connected() {
            let _ = app.send_frame();
        }

        render(terminal, &mut app)?;
        handle_input(&mut app)?;
        std::thread::sleep(tick_rate);
    }

    app.stop_comm_thread();
    Ok(())
}

fn render(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut app::App) -> io::Result<()> {
    terminal.draw(|frame| {
        ui::render(frame, app);
    })?;
    Ok(())
}

/// 输入事件处理
///
/// # Arguments
///
/// * `app` - 应用状态
///
/// # Returns
///
/// Result<(), Error> - 错误信息
fn handle_input(app: &mut app::App) -> io::Result<()> {
    if event::poll(Duration::from_millis(10))? {
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                // 编辑模式优先处理
                if app.in_edit_mode {
                    handle_edit_mode_input(app, &key);
                } else if app.in_servo_mode {
                    let evt = handle_joint_mode_input(key.code);
                    input::handle_event(app, evt);
                } else if app.in_settings {
                    let evt = handle_settings_mode_input(key.code);
                    input::handle_event(app, evt);
                } else if app.popup.is_visible() {
                    let evt = handle_comm_popup_input(key.code, app);
                    input::handle_event(app, evt);
                } else {
                    let evt = handle_menu_mode_input(key.code, key.modifiers, app);
                    input::handle_event(app, evt);
                }
            }
        }
    }
    Ok(())
}

/// popup界面事件处理
///
/// # Arguments
///
/// * `code` - 键码
/// * `app` - 应用状态
///
/// # Returns
///
/// AppEvent - 应用事件
fn handle_comm_popup_input(code: KeyCode, app: &mut app::App) -> input::AppEvent {
    use input::CommonEvent;
    match code {
        KeyCode::Esc => {
            app.stop_comm_thread();
            CommonEvent::None.into()
        }
        _ => CommonEvent::None.into(),
    }
}

/// 关节界面消息处理
///
/// # Arguments
///
/// * `code` - 键码
///
/// # Returns
///
/// AppEvent - 应用事件
fn handle_joint_mode_input(code: KeyCode) -> input::AppEvent {
    use input::{CommonEvent, DeviceEvent};
    match code {
        KeyCode::Esc => DeviceEvent::Exit.into(),
        KeyCode::Up => DeviceEvent::Prev.into(),
        KeyCode::Down => DeviceEvent::Next.into(),
        KeyCode::Left => DeviceEvent::Decrease.into(),
        KeyCode::Right => DeviceEvent::Increase.into(),
        KeyCode::Char('s') => DeviceEvent::Screenshot.into(),
        _ => CommonEvent::None.into(),
    }
}

/// 设置界面按键处理
///
/// # Arguments
///
/// * `code` - 键码
///
/// # Returns
///
/// AppEvent - 应用事件
fn handle_settings_mode_input(code: KeyCode) -> input::AppEvent {
    use input::{CommonEvent, SettingsEvent};
    match code {
        KeyCode::Esc => SettingsEvent::Exit.into(),
        KeyCode::Up => SettingsEvent::Up.into(),
        KeyCode::Down => SettingsEvent::Down.into(),
        KeyCode::Enter => SettingsEvent::EnterEdit.into(),
        _ => CommonEvent::None.into(),
    }
}

/// 设置界面中编辑内容时按键处理
///
/// # Arguments
///
/// * `app` - 应用状态
/// * `key` - 键盘事件
fn handle_edit_mode_input(app: &mut app::App, key: &event::KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.in_edit_mode = false;
            app.edit_buffer.clear();
        }
        KeyCode::Enter => {
            // 保存
            match app.settings_selected {
                0 => app.config.wifi_ssid = app.edit_buffer.clone(),
                1 => app.config.wifi_password = app.edit_buffer.clone(),
                2 => app.config.speech_name = app.edit_buffer.clone(),
                _ => {}
            }
            if let Err(e) = app.config.save() {
                log::error!("Failed to save settings: {e}");
            }
            app.in_edit_mode = false;
            app.edit_buffer.clear();
        }
        KeyCode::Backspace => {
            app.edit_buffer.pop();
        }
        KeyCode::Char(c) => {
            app.edit_buffer.push(c);
        }
        _ => {}
    }
}

/// 侧边菜单栏事件处理
///
/// # Arguments
///
/// * `code` - 键码
/// * `modifiers` - 修饰键
/// * `app` - 应用状态
///
/// # Returns
///
/// AppEvent - 应用事件
fn handle_menu_mode_input(
    code: KeyCode,
    modifiers: KeyModifiers,
    app: &app::App,
) -> input::AppEvent {
    use input::{CommonEvent, MenuEvent, SettingsEvent};

    match code {
        KeyCode::Esc => CommonEvent::Quit.into(),
        KeyCode::Up => MenuEvent::Up.into(),
        KeyCode::Down => MenuEvent::Down.into(),
        KeyCode::Char('s') => {
            if modifiers == KeyModifiers::CONTROL {
                SettingsEvent::Save.into()
            } else {
                CommonEvent::None.into()
            }
        }
        KeyCode::Enter => {
            if matches!(app.selected_menu, app::MenuItem::DeviceControl) {
                MenuEvent::EnterServoMode.into()
            } else if matches!(app.selected_menu, app::MenuItem::Settings) {
                MenuEvent::EnterSettingMode.into()
            } else {
                MenuEvent::ConnectDevice.into()
            }
        }
        _ => CommonEvent::None.into(),
    }
}
