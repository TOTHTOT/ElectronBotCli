extern crate log;

mod app;
mod emotion;
mod input;
mod llm;
mod media;
mod model_manager;
mod robot;
mod ui;
mod ui_components;
mod vision;
mod web;

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
    if let Err(e) = run(&mut terminal) {
        log::error!("app failed: {e}");
        return Err(e);
    }
    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;

    Ok(())
}

/// 主线程
/// 实现页面渲染, 按键事件, 机器人数据发送
fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> anyhow::Result<()> {
    let mut app = app::App::new()?;

    let tick_rate = Duration::from_millis(20);
    while app.running {
        if app.is_connected() {
            let _ = app.send_frame();
        }
        app.poll_voice_input();
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

/// 输入事件处理入口, 会根据当前页面路由到对应事件
///
/// # Arguments
///
/// * `app`:
///
/// returns: Result<(), Error>
///
/// # Examples
///
/// ```
///
/// ```
fn handle_input(app: &mut app::App) -> io::Result<()> {
    if !event::poll(Duration::from_millis(10))? {
        return Ok(());
    }

    if let Event::Key(key) = event::read()? {
        if key.kind != KeyEventKind::Press {
            return Ok(());
        }

        // 全局快捷键
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('q') {
            app.quit();
            return Ok(());
        }
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('s') {
            if let Err(e) = app.config.save() {
                log::error!("Failed to save settings: {e}");
            }
            return Ok(());
        }

        // 分发到输入模块处理
        input::handle_by_mode(app, key.code, key.modifiers);
    }
    Ok(())
}
