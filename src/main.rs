extern crate log;

mod app;
mod device;
mod event_handler;
mod ui;
mod ui_components;

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
    // 初始化日志 (每次启动截断日志文件)
    let log_file = File::create("ele_bot.log").ok();
    if let Some(f) = log_file {
        CombinedLogger::init(vec![WriteLogger::new(
            simplelog::LevelFilter::Info,
            Config::default(),
            f,
        )])
        .ok();
    }

    let mut stdout = io::stdout();
    enable_raw_mode()?;
    stdout.execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;
    run(&mut terminal)?;
    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;

    Ok(())
}

fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> anyhow::Result<()> {
    let mut app = app::App::new();
    app.load_image_from_file("./assets/images/test.png")?;
    let tick_rate = Duration::from_millis(20);

    while app.running {
        // 如果已连接，更新帧数据到共享状态
        if app.is_connected() {
            app.build_send_frame();
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
                    handle_servo_mode_input(key.code)
                } else if app.port_select_popup.is_visible() {
                    handle_port_select_input(key.code, app)
                } else {
                    handle_menu_mode_input(key.code, app)
                };
                event_handler::handle_event(app, evt);
            }
        }
    }
    Ok(())
}

fn handle_port_select_input(code: KeyCode, app: &mut app::App) -> event_handler::AppEvent {
    match code {
        KeyCode::Esc => {
            app.port_select_popup.hide();
            event_handler::AppEvent::None
        }
        KeyCode::Enter => {
            if let Some(port) = app.port_select_popup.selected_port() {
                let port_name = port.to_string();
                app.start_comm_thread(&port_name);
            }
            app.port_select_popup.hide();
            event_handler::AppEvent::None
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.port_select_popup.prev();
            event_handler::AppEvent::None
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.port_select_popup.next();
            event_handler::AppEvent::None
        }
        _ => event_handler::AppEvent::None,
    }
}

fn handle_servo_mode_input(code: KeyCode) -> event_handler::AppEvent {
    match code {
        KeyCode::Char('q') | KeyCode::Esc => event_handler::AppEvent::ExitServoMode,
        KeyCode::Up | KeyCode::Char('k') => event_handler::AppEvent::ServoIncrease,
        KeyCode::Down | KeyCode::Char('j') => event_handler::AppEvent::ServoDecrease,
        KeyCode::Char('h') => event_handler::AppEvent::ServoPrev,
        KeyCode::Char('l') => event_handler::AppEvent::ServoNext,
        KeyCode::Char('a') => event_handler::AppEvent::ServoDecreaseBig,
        KeyCode::Char('d') => event_handler::AppEvent::ServoIncreaseBig,
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
