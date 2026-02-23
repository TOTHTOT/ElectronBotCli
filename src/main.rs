extern crate log;

mod app;
mod emotion;
mod input;
mod llm;
mod robot;
mod ui;
mod ui_components;
mod voice;

use crate::llm::QwenLlm;
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
            simplelog::LevelFilter::Info,
            Config::default(),
            f,
        )])
        .ok();
    }

    // 初始化 LLM
    let llm = init_llm();
    if llm.is_some() {
        log::info!("LLM initialized successfully");
    } else {
        log::warn!("LLM init failed, running without LLM");
    }

    let voice_manager =
        VoiceManager::new("assets/module/vosk/vosk-model-small-cn-0.22", "麦克风阵列").ok();

    let mut stdout = io::stdout();
    enable_raw_mode()?;
    stdout.execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;
    run(&mut terminal, voice_manager, llm)?;
    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;

    Ok(())
}

/// 初始化 LLM
fn init_llm() -> Option<QwenLlm> {
    let mut llm = QwenLlm::load("assets/module/llm/qwen2/qwen2.5-0.5b-instruct-q4_0.gguf").ok()?;
    llm.load_tokenizer("assets/module/llm/qwen2/tokenizer.json").ok()?;
    llm.preload().ok()?;
    Some(llm)
}

/// 主运行循环，负责应用的生命周期管理
fn run(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    voice_manager: Option<VoiceManager>,
    llm: Option<QwenLlm>,
) -> anyhow::Result<()> {
    let mut app = app::App::new(voice_manager, llm);

    let tick_rate = Duration::from_millis(20);
    while app.running {
        // 处理语音输入（在 app 内部处理）
        app.poll_voice_input();

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
