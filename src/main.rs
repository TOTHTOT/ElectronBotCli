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
use std::io::{self, Stdout};
use std::time::Duration;

fn main() -> io::Result<()> {
    let mut stdout = io::stdout();
    enable_raw_mode()?;
    stdout.execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;

    let result = run(&mut terminal);

    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;

    result
}

fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
    let mut app = app::App::new();
    let tick_rate = Duration::from_millis(50);
    let mut log_popup = ui_components::LogPopup::new();

    while app.running {
        render(terminal, &mut app, &mut log_popup)?;
        handle_input(&mut app, &mut log_popup)?;

        if app.device.is_connected() {
            app.send_frame();
        }

        std::thread::sleep(tick_rate);
    }

    app.disconnect_device();
    Ok(())
}

fn render(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut app::App,
    log_popup: &mut ui_components::LogPopup,
) -> io::Result<()> {
    terminal.draw(|frame| {
        ui::render(frame, app);
        log_popup.render(frame, frame.area(), &mut app.log_queue);
    })?;
    Ok(())
}

fn handle_input(app: &mut app::App, log_popup: &mut ui_components::LogPopup) -> io::Result<()> {
    if event::poll(Duration::from_millis(10))? {
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                let evt = if app.in_servo_mode {
                    handle_servo_mode_input(key.code)
                } else {
                    handle_menu_mode_input(key.code, app, log_popup)
                };
                event_handler::handle_event(app, evt);
            }
        }
    }
    Ok(())
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

fn handle_menu_mode_input(
    code: KeyCode,
    app: &app::App,
    log_popup: &mut ui_components::LogPopup,
) -> event_handler::AppEvent {
    match code {
        KeyCode::Char('q') | KeyCode::Esc => event_handler::AppEvent::Quit,
        KeyCode::Up | KeyCode::Char('k') => event_handler::AppEvent::MenuUp,
        KeyCode::Down | KeyCode::Char('j') => event_handler::AppEvent::MenuDown,
        KeyCode::Enter => {
            if matches!(app.selected_menu, app::MenuItem::DeviceControl) {
                event_handler::AppEvent::EnterServoMode
            } else {
                event_handler::AppEvent::ConnectDevice
            }
        }
        KeyCode::Char('l') => {
            log_popup.toggle();
            event_handler::AppEvent::None
        }
        _ => event_handler::AppEvent::None,
    }
}
