use crate::app::BotRecvType;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// 通信线程状态
pub struct CommState {
    pub running: Arc<AtomicBool>,
}

/// 启动通信线程
pub fn start_comm_thread(
    port_name: String,
    rx: mpsc::Receiver<BotRecvType>,
) -> (CommState, thread::JoinHandle<()>) {
    let running = Arc::new(AtomicBool::new(true));
    let state = CommState {
        running: running.clone(),
    };

    let handle = thread::spawn(move || {
        let result = serialport::new(&port_name, 3000000)
            .timeout(Duration::from_millis(100))
            .open();

        match result {
            Ok(mut port) => {
                port.write_data_terminal_ready(true).ok();
                port.write_request_to_send(true).ok();
                thread::sleep(Duration::from_millis(50));
                // 接收数据并发送
                for (pixels, joint) in rx {
                    if !running.load(Ordering::Relaxed) {
                        break;
                    }
                    let config_array: [u8; 32] = joint.to_bytes();
                    log::info!(
                        "Sending {} pixels, config_array: {config_array:?}",
                        pixels.len()
                    );
                    send_frame_data(&mut port, &pixels, &config_array);
                }

                log::info!("Communication thread stopped");
            }
            Err(e) => {
                log::error!("Failed to connect: {e}");
            }
        }

        running.store(false, Ordering::Relaxed);
    });

    (state, handle)
}

/// 停止通信线程
pub fn stop_comm_thread(state: &CommState) {
    state.running.store(false, Ordering::Relaxed);
}

/// 发送帧数据到串口
fn send_frame_data(
    port: &mut Box<dyn serialport::SerialPort>,
    pixels: &[u8],
    config_bytes: &[u8; 32],
) {
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
