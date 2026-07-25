//! WebSocket 服务
//!
//! 接受客户端连接, 接收命令, 推送事件 (从 `EventBus` 订阅, 按 variant 过滤).

use crate::event_bus::BusEvent;
use crate::robot::{self, CommState, DisplayMode, JointConfig};
use crate::state::{mood_from_proto, SharedState};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use ele_bot_proto::{ClientMessage, ServerEvent, SERVO_COUNT};
use futures::{SinkExt, StreamExt};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

const WS_PATH: &str = "/ws";

/// 启动 WebSocket 服务(阻塞)
pub async fn run(state: Arc<SharedState>, bind: &str) -> anyhow::Result<()> {
    let app = Router::new()
        .route(WS_PATH, get(ws_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(bind).await?;
    log::info!("WebSocket server listening on {bind}{WS_PATH}");

    axum::serve(listener, app).await?;
    Ok(())
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<SharedState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_connection(socket, state))
}

async fn handle_connection(socket: WebSocket, state: Arc<SharedState>) {
    let (mut ws_tx, mut ws_rx) = socket.split();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<ServerEvent>();
    log::info!("WebSocket connection established");
    // 发送任务: 把 out_rx 事件序列化为 WS 文本帧
    let send_task = tokio::spawn(async move {
        while let Some(evt) = out_rx.recv().await {
            let payload = match evt.to_json() {
                Ok(s) => s,
                Err(e) => {
                    log::warn!("serialize event failed: {e}");
                    continue;
                }
            };
            if ws_tx.send(Message::Text(payload.into())).await.is_err() {
                break;
            }
        }
    });

    // 订阅任务: 从 EventBus 过滤该外发给 WS 的事件, 转发到 out_tx.
    // BusEvent::ServerEvent 和 Volume 直接 / 转译外发; AsrText/LlmReply/
    // LlmProcessing 是内部用, 不外发.
    let mut bus_rx = state.bus_tx.subscribe();
    let out_tx_clone = out_tx.clone();
    let sub_task = tokio::spawn(async move {
        loop {
            match bus_rx.recv().await {
                Ok(BusEvent::ServerEvent(se)) => {
                    if out_tx_clone.send(se).is_err() {
                        break;
                    }
                }
                Ok(BusEvent::Volume(v)) => {
                    if out_tx_clone
                        .send(ele_bot_proto::ServerEvent::Volume { value: v })
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(_) => continue, // AsrText/LlmReply/LlmProcessing 内部用, 不外发
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    log::debug!("ws bus lagged, dropped {n}");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // 推初始状态
    let _ = out_tx.send(ServerEvent::Config {
        config: state.config(),
    });
    let _ = out_tx.send(ServerEvent::Connection {
        is_connected: state.robot_connected.load(Ordering::Relaxed),
    });
    state.broadcast_joint_state();
    state.broadcast_joint_config();

    // 主循环
    let mut frame_interval = tokio::time::interval(Duration::from_millis(50));
    frame_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            biased;

            msg = ws_rx.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        match ClientMessage::from_json(&text) {
                            Ok(cmd) => {
                                if let Err(e) = handle_command(&state, cmd, &out_tx).await {
                                    log::warn!("handle command error: {e}");
                                }
                            }
                            Err(e) => {
                                log::warn!("invalid client message: {e}");
                                if let Err(e) = out_tx.send(ServerEvent::Error {
                                    message: e.to_string(),
                                }){
                                    log::warn!("error sending error: {e}");
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(e)) => {
                        log::warn!("ws error: {e}");
                        break;
                    }
                    _ => {}
                }
            }

            _ = frame_interval.tick() => {
                if state.robot_connected.load(Ordering::Relaxed) {
                    let pixels = state.generate_lcd_frame();
                    if !pixels.is_empty() {
                        state.push_frame_to_robot(pixels.clone());
                        // 写 web preview 缓存
                        if let Ok(mut guard) = state.lcd_frame_cache.lock() {
                            *guard = Some(pixels);
                        }
                    }
                }
                // 音量推送: 由 voice 主动 publish BusEvent::Volume, ws.rs
                // 通过 sub_task 订阅 BusEvent::Volume 转 ServerEvent::Volume
                // 外发. 这里不再 50ms tick 轮询.
            }
        }
    }

    drop(out_tx);
    let _ = send_task.await;
    let _ = sub_task.await;
}

async fn handle_command(
    state: &Arc<SharedState>,
    cmd: ClientMessage,
    out_tx: &mpsc::UnboundedSender<ServerEvent>,
) -> anyhow::Result<()> {
    log::info!("received command: {cmd:?}");
    match cmd {
        ClientMessage::Ping => {
            out_tx.send(ServerEvent::Pong)?;
        }
        ClientMessage::GetConfig => {
            out_tx.send(ServerEvent::Config {
                config: state.config(),
            })?;
        }
        ClientMessage::SetConfig { config } => {
            state.set_config(config)?;
        }
        ClientMessage::ConnectRobot => {
            state.stop_robot_comm();
            let (tx, rx) = std::sync::mpsc::sync_channel::<(Vec<u8>, JointConfig)>(1);
            *state.bot_tx.lock().unwrap() = Some(tx);
            match robot::start_comm_thread(rx) {
                Ok((comm_state, _handle)) => {
                    *state.comm_state.lock().unwrap() = Some(comm_state);
                    state.notify_connection(true);
                }
                Err(e) => {
                    log::warn!("failed to connect: {e}");
                    *state.bot_tx.lock().unwrap() = None;
                    state.notify_connection(false);
                }
            }
        }
        ClientMessage::DisconnectRobot => {
            state.stop_robot_comm();
            state.notify_connection(false);
        }
        ClientMessage::SetJoint { servo_index, angle } => {
            if (servo_index as usize) < SERVO_COUNT {
                state.joint.set_angle(servo_index as usize, angle);
                state.broadcast_joint_state();
            }
        }
        ClientMessage::SetJoints { angles } => {
            for (i, a) in angles.iter().enumerate() {
                state.joint.set_angle(i, *a);
            }
            state.broadcast_joint_state();
        }
        ClientMessage::SelectServo { delta } => {
            if delta > 0 {
                state.joint.next_servo();
            } else if delta < 0 {
                state.joint.prev_servo();
            }
            state.broadcast_joint_state();
        }
        ClientMessage::AdjustSelectedServo { delta } => {
            if delta > 0 {
                state.joint.increase();
            } else if delta < 0 {
                state.joint.decrease();
            }
            state.broadcast_joint_state();
        }
        ClientMessage::SetMood { mood } => {
            state.set_mood(mood_from_proto(mood));
        }
        ClientMessage::SetLcdMode { mode } => {
            let proto_mode = match mode {
                ele_bot_proto::DisplayMode::Static => DisplayMode::Static,
                ele_bot_proto::DisplayMode::Eyes => DisplayMode::Eyes,
                ele_bot_proto::DisplayMode::TestPattern => DisplayMode::TestPattern,
            };
            if let Ok(mut lcd) = state.lcd.lock() {
                lcd.set_mode(proto_mode);
            }
        }
        ClientMessage::SetFaceTracking { enabled } => {
            state.set_face_tracking(enabled);
        }
        ClientMessage::SendLlmText { text } => {
            // 走 EventBus, 跟 ASR 路径统一. LLM task 订阅 BusEvent::AsrText.
            state.bus_tx.publish(BusEvent::AsrText(text));
        }
        ClientMessage::TtsSpeak {
            text,
            speed,
            streaming,
        } => {
            if let Some(voice) = state.voice.lock().unwrap().clone() {
                let text_clone = text.clone();
                // 异步播放, 不阻塞 ws 任务
                tokio::task::spawn_blocking(move || {
                    let result = if streaming {
                        voice.speak_streaming(&text_clone, speed, None)
                    } else {
                        voice.speak(&text_clone, speed, None)
                    };
                    if let Err(e) = result {
                        log::warn!("TTS playback failed: {e:?}");
                    }
                });
            } else {
                let _ = out_tx.send(ServerEvent::Error {
                    message: "voice manager not available".to_string(),
                });
            }
        }
        ClientMessage::TakeScreenshot => {
            let path = state.take_screenshot()?;
            let _ = out_tx.send(ServerEvent::ScreenshotSaved { path });
        }
        ClientMessage::ListInputDevices => {
            let devices = crate::media::voice::list_input_devices_dto();
            let _ = out_tx.send(ServerEvent::InputDevices { devices });
        }
        ClientMessage::ListOutputDevices => {
            let devices = crate::media::voice::list_output_devices_dto();
            let _ = out_tx.send(ServerEvent::OutputDevices { devices });
        }
    }
    Ok(())
}

// 让 CommState 类型被使用(防止 lib 警告)
#[allow(dead_code)]
fn _comm_state_marker(_: &CommState) {}
