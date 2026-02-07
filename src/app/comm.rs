use crate::app::{FRAME_HEIGHT, FRAME_WIDTH};
use crate::device::JointConfig;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// 共享状态用于后台线程
pub struct SharedState {
    pub running: Arc<AtomicBool>,
    pub pixels: Mutex<Vec<u8>>,
    pub config: Mutex<JointConfig>,
}

impl SharedState {
    pub fn new(pixels: Vec<u8>, config: JointConfig) -> Self {
        Self {
            running: Arc::new(AtomicBool::new(true)),
            pixels: Mutex::new(pixels),
            config: Mutex::new(config),
        }
    }
}

/// 发送帧数据到串口
pub fn send_frame_data(port: &mut Box<dyn serialport::SerialPort>, pixels: &[u8], config_bytes: &[u8; 32]) {
    let mut frame_offset = 0;

    for _ in 0..4 {
        // 发送 84*512 字节
        for i in 0..84 {
            let chunk_offset = frame_offset + i * 512;
            let _ = port.write(&pixels[chunk_offset..chunk_offset + 512]);
        }
        frame_offset += 84 * 512;

        // 发送尾部
        let mut tail_packet = [0u8; 224];
        tail_packet[0..192].copy_from_slice(&pixels[frame_offset..frame_offset + 192]);
        tail_packet[192..].copy_from_slice(config_bytes);
        let _ = port.write(&tail_packet);
        frame_offset += 192;

        // 读取响应
        let mut data = [0u8; 32];
        let _ = port.read_exact(&mut data);
    }
}

/// 通信线程处理函数
pub fn comm_thread_handler(port_name: String, shared: Arc<SharedState>) {
    let running = shared.running.clone();

    thread::spawn(move || {
        let result = serialport::new(&port_name, 115200)
            .timeout(Duration::from_millis(100))
            .open();

        match result {
            Ok(mut port) => {
                port.write_data_terminal_ready(true).ok();
                port.write_request_to_send(true).ok();
                thread::sleep(Duration::from_millis(50));

                log::info!("Communication thread started");

                let mut ping_pong_index = 0;

                while running.load(Ordering::Relaxed) {
                    ping_pong_index = if ping_pong_index == 0 { 1 } else { 0 };

                    // 获取当前像素和配置
                    let pixels = if let Ok(p) = shared.pixels.lock() {
                        p.clone()
                    } else {
                        vec![0u8; FRAME_WIDTH * FRAME_HEIGHT * 3]
                    };
                    let config_bytes = if let Ok(config) = shared.config.lock() {
                        config.to_bytes()
                    } else {
                        [0u8; 32]
                    };

                    // 发送帧数据
                    send_frame_data(&mut port, &pixels, &config_bytes);

                    thread::sleep(Duration::from_millis(10));
                }

                log::info!("Communication thread stopped");
            }
            Err(e) => {
                log::error!("Failed to connect: {e}");
            }
        }
    });
}
