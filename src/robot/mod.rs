//! Robot 模块 - ElectronBot 机器人抽象
//!
//! 使用 [electron_bot](electron_bot/index.html) 库实现 USB 通信

pub mod joint;
pub mod lcd;

pub use joint::{Joint, JointConfig, ServoState, SERVO_COUNT};
pub use lcd::{DisplayMode, Lcd};

use electron_bot::ElectronBot;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
// ==================== Robot 结构体 ====================

/// ElectronBot 机器人（封装 electron_bot 库）
#[allow(dead_code)]
pub struct Robot {
    bot: ElectronBot,
}

#[allow(dead_code)]
impl Robot {
    /// 打开并初始化机器人连接
    pub fn open() -> Result<Self, electron_bot::BotError> {
        let mut bot = ElectronBot::new();
        bot.connect()?;
        Ok(Self { bot })
    }

    /// 检查是否已连接
    pub fn is_connected(&self) -> bool {
        self.bot.is_connected()
    }

    /// 发送一帧数据（像素 + 关节配置）
    pub fn send_frame(
        &mut self,
        pixels: &[u8],
        config: &[u8; 32],
    ) -> Result<(), electron_bot::BotError> {
        // 更新图片缓冲区
        self.bot
            .image_buffer()
            .as_mut_data()
            .copy_from_slice(pixels);

        // 更新扩展数据（关节角度）
        self.bot.extra_data().set_raw(config);

        // 同步到设备
        self.bot.sync()?;
        Ok(())
    }
}

// ==================== 通信线程管理 ====================

/// 通信线程状态
pub struct CommState {
    pub running: Arc<AtomicBool>,
}

/// 启动后台通信线程
pub fn start_comm_thread(
    rx: std::sync::mpsc::Receiver<(Vec<u8>, JointConfig)>,
) -> anyhow::Result<(CommState, thread::JoinHandle<()>)> {
    let running = Arc::new(AtomicBool::new(true));
    let state = CommState {
        running: running.clone(),
    };

    let mut bot = ElectronBot::new();
    match bot.connect() {
        Ok(_) => {
            log::info!("Robot connected");
        }
        Err(e) => {
            anyhow::bail!("Failed to connect: {e}");
        }
    }
    let handle = thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));

        // 主循环
        for (pixels, joint) in rx {
            if !running.load(Ordering::Relaxed) {
                break;
            }

            // 更新图片缓冲区
            bot.image_buffer().as_mut_data().copy_from_slice(&pixels);

            // 更新扩展数据（关节角度）
            bot.extra_data().set_raw(&joint.as_bytes());
            // 同步到设备
            if let Err(e) = bot.sync() {
                log::error!("Sync failed: {e}");
            }
        }

        // 停止舵机
        let stop_config = JointConfig::default();
        bot.extra_data().set_raw(&stop_config.as_bytes());
        let _ = bot.sync();

        bot.disconnect();
        log::info!("Communication stopped");
        running.store(false, Ordering::Relaxed);
    });

    Ok((state, handle))
}

/// 停止通信线程
pub fn stop_comm_thread(state: &CommState) {
    state.running.store(false, Ordering::Relaxed);
}

// ==================== 便捷函数 ====================

#[allow(dead_code)]
/// 扫描 USB 设备
pub fn scan_devices() -> Vec<(u16, u16, String)> {
    ElectronBot::scan_devices()
        .into_iter()
        .map(|d| (d.vid, d.pid, d.info))
        .collect()
}

#[allow(dead_code)]
/// 检查设备是否已连接
pub fn is_device_present() -> bool {
    ElectronBot::is_device_present()
}
