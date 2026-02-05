mod app;
mod device;
mod event_handler;
mod ui;

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::prelude::*;
use std::io;
use std::time::Duration;

fn main() -> io::Result<()> {
    // 初始化终端
    enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;

    let result = run(&mut terminal);

    // 恢复终端
    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;

    result
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    let mut app = app::App::new();
    let tick_rate = Duration::from_millis(50); // 20 FPS

    // 启动设备发送线程
    app.start_device_thread();

    while app.running {
        // 渲染界面
        terminal.draw(|frame| ui::render(frame, &mut app))?;

        // 非阻塞读取事件
        if event::poll(Duration::from_millis(10))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    let evt = if app.in_servo_mode {
                        // 舵机模式下的按键处理
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => {
                                event_handler::AppEvent::ExitServoMode
                            }
                            KeyCode::Up | KeyCode::Char('k') => {
                                event_handler::AppEvent::ServoIncrease
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                event_handler::AppEvent::ServoDecrease
                            }
                            KeyCode::Char('h') => event_handler::AppEvent::ServoPrev,
                            KeyCode::Char('l') => event_handler::AppEvent::ServoNext,
                            KeyCode::Char('a') => event_handler::AppEvent::ServoDecreaseBig,
                            KeyCode::Char('d') => event_handler::AppEvent::ServoIncreaseBig,
                            _ => event_handler::AppEvent::None,
                        }
                    } else {
                        // 普通菜单模式下的按键处理
                        match key.code {
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
                            _ => event_handler::AppEvent::None,
                        }
                    };
                    event_handler::handle_event(&mut app, evt);
                }
            }
        }

        // 如果设备已连接，定时发送帧数据, 得通过线程发送, 不然影响刷新速度
        if app.device.is_connected() {
            let frame = app.create_test_frame();
            app.send_frame(&frame);
        }

        // 控制帧率
        std::thread::sleep(tick_rate);
    }

    // 停止设备线程并断开连接
    if let Some(sender) = &app.device_sender {
        sender.stop();
        sender.disconnect();
    }

    Ok(())
}
