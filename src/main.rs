extern crate log;

mod app;
mod event_handler;
mod robot;
mod ui;
mod ui_components;
mod voice;

use crate::voice::VoiceManager;
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

fn run(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    voice_manager: VoiceManager,
) -> anyhow::Result<()> {
    let mut app = app::App::new(voice_manager);
    let tick_rate = Duration::from_millis(20);
    while app.running {
        // 如果已连接，隐藏连接弹窗
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

fn handle_input(app: &mut app::App) -> io::Result<()> {
    if event::poll(Duration::from_millis(10))? {
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                let evt = if app.in_servo_mode {
                    handle_joint_mode_input(key.code)
                } else if app.popup.is_visible() {
                    handle_comm_popup_input(key.code, app)
                } else {
                    handle_menu_mode_input(key.code, app)
                };
                event_handler::handle_event(app, evt);
            }
        }
    }
    Ok(())
}

fn handle_comm_popup_input(code: KeyCode, app: &mut app::App) -> event_handler::AppEvent {
    match code {
        KeyCode::Esc => {
            app.stop_comm_thread();
            event_handler::AppEvent::None
        }
        _ => event_handler::AppEvent::None,
    }
}

fn handle_joint_mode_input(code: KeyCode) -> event_handler::AppEvent {
    match code {
        KeyCode::Esc => event_handler::AppEvent::ExitServoMode,
        KeyCode::Up => event_handler::AppEvent::ServoPrev,
        KeyCode::Down => event_handler::AppEvent::ServoNext,
        KeyCode::Left => event_handler::AppEvent::ServoDecrease,
        KeyCode::Right => event_handler::AppEvent::ServoIncrease,
        KeyCode::Char('s') => event_handler::AppEvent::Screenshot,
        _ => event_handler::AppEvent::None,
    }
}

fn handle_menu_mode_input(code: KeyCode, _app: &app::App) -> event_handler::AppEvent {
    match code {
        KeyCode::Char('q') | KeyCode::Esc => event_handler::AppEvent::Quit,
        KeyCode::Up | KeyCode::Char('k') => event_handler::AppEvent::MenuUp,
        KeyCode::Down | KeyCode::Char('j') => event_handler::AppEvent::MenuDown,
        KeyCode::Enter => {
            if matches!(_app.selected_menu, app::MenuItem::DeviceControl) {
                event_handler::AppEvent::EnterServoMode
            } else {
                event_handler::AppEvent::ConnectDevice
            }
        }
        _ => event_handler::AppEvent::None,
    }
}
