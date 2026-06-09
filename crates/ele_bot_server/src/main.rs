//! ElectronBotCli 服务端入口
//!
//! 启动流程:
//! 1. 解析命令行参数(端口、绑定地址)
//! 2. 设置日志
//! 3. 执行测试模式(若设置环境变量), 否则正常运行
//! 4. 初始化所有硬件资源
//! 5. 启动 WebSocket 服务, 等待客户端连接

use std::fs::File;
use std::io;

use simplelog::{CombinedLogger, Config, WriteLogger};

use ele_bot_server::state::SharedState;
use ele_bot_server::test_mode;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // panic hook
    std::panic::set_hook(Box::new(|info| {
        eprintln!("panic: {info}");
    }));

    // 日志
    if let Ok(f) = File::create("server.log") {
        let _ = CombinedLogger::init(vec![WriteLogger::new(
            simplelog::LevelFilter::Info,
            Config::default(),
            f,
        )]);
    }

    // 解析参数
    let args: Vec<String> = std::env::args().collect();
    let mut bind = "0.0.0.0:7878".to_string();
    for i in 1..args.len() {
        if args[i] == "--bind" && i + 1 < args.len() {
            bind = args[i + 1].clone();
        }
    }

    // 测试模式(由环境变量触发)
    if test_mode::run_test_mode()? {
        return Ok(());
    }

    log::info!("starting ele_bot_server on {}", bind);

    // 初始化硬件
    let state = SharedState::new()?;
    log::info!("hardware initialized");

    // 启动 WebSocket 服务
    ele_bot_server::ws::run(state, &bind).await?;

    Ok(())
}

#[allow(dead_code)]
fn _silence_io(_: io::Empty) {}
