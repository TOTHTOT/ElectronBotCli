//! ElectronBotCli 客户端入口
//!
//! 默认连接 ws://127.0.0.1:7878/ws, 也可通过环境变量 SERVER_URL 自定义。

use std::env;
use std::fs::File;
use std::io::{self, Stdout};
use std::panic;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::prelude::*;
use simplelog::{CombinedLogger, Config, WriteLogger};

use ele_bot_client::app::App;

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = io::stdout().execute(LeaveAlternateScreen);
    }
}

fn main() -> anyhow::Result<()> {
    panic::set_hook(Box::new(|_| {
        let _ = disable_raw_mode();
        let _ = io::stdout().execute(LeaveAlternateScreen);
    }));

    let _guard = TerminalGuard;

    if let Ok(f) = File::create("client.log") {
        let _ = CombinedLogger::init(vec![WriteLogger::new(
            simplelog::LevelFilter::Info,
            Config::default(),
            f,
        )]);
    }

    let server_url =
        env::var("SERVER_URL").unwrap_or_else(|_| "ws://127.0.0.1:7878/ws".to_string());
    log::info!("connecting to {}", server_url);

    let mut app = App::new(&server_url)?;

    let mut stdout = io::stdout();
    enable_raw_mode()?;
    stdout.execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;

    run(&mut terminal, &mut app)?;

    Ok(())
}

fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> io::Result<()> {
    let tick_rate = Duration::from_millis(20);
    while app.ui.running {
        // 拉服务端事件
        app.poll_events();
        // 拉 LLM/语音事件(本地状态镜像)
        app.poll_voice_input();

        render(terminal, app)?;
        handle_input(app)?;
        std::thread::sleep(tick_rate);
    }
    Ok(())
}

fn render(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> io::Result<()> {
    terminal.draw(|frame| {
        ele_bot_client::ui::render(frame, app);
    })?;
    Ok(())
}

fn handle_input(app: &mut App) -> io::Result<()> {
    if !event::poll(Duration::from_millis(10))? {
        return Ok(());
    }
    if let Event::Key(key) = event::read()? {
        if key.kind != KeyEventKind::Press {
            return Ok(());
        }

        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('q') {
            app.quit();
            return Ok(());
        }
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('s') {
            // 配置通过 SetConfig 命令保存
            app.set_config(app.config.clone());
            return Ok(());
        }

        ele_bot_client::input::handle_by_mode(app, key.code, key.modifiers);
    }
    Ok(())
}
