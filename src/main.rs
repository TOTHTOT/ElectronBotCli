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

/// 主运行循环，负责应用的生命周期管理
///
/// 循环执行以下步骤：
/// 1. 如果已连接机器人，发送当前帧数据
/// 2. 渲染UI界面
/// 3. 处理用户输入
/// 4. 休眠一个tick周期
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

/// 输入事件处理入口
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
