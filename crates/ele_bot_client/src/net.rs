//! WebSocket 客户端
//!
//! 封装与服务端的 WebSocket 连接, 提供发送命令和接收事件的 API。
//! 内部用 tokio 任务处理 IO, 暴露同步 API 供 App 在主循环中调用。

use ele_bot_proto::{ClientMessage, ServerEvent};
use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex as TokioMutex};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::MaybeTlsStream;

/// 网络客户端
pub struct Client {
    /// 发送命令的通道
    tx: mpsc::UnboundedSender<ClientMessage>,
    /// 接收事件的通道(从后台读取任务填充)
    rx: Arc<TokioMutex<mpsc::UnboundedReceiver<ServerEvent>>>,
}

impl Client {
    /// 连接到 WebSocket 服务器
    pub async fn connect(url: &str) -> anyhow::Result<Self> {
        let (ws_stream, _resp) = tokio_tungstenite::connect_async(url).await?;
        let (mut ws_tx, mut ws_rx) = ws_stream.split::<Message>();

        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<ClientMessage>();
        let (evt_tx, evt_rx) = mpsc::unbounded_channel::<ServerEvent>();

        // 发送任务: 从 cmd_rx 读取命令, 通过 WS 发送
        tokio::spawn(async move {
            while let Some(cmd) = cmd_rx.recv().await {
                let json = match cmd.to_json() {
                    Ok(s) => s,
                    Err(e) => {
                        log::warn!("serialize client message failed: {e}");
                        continue;
                    }
                };
                if ws_tx.send(Message::Text(json)).await.is_err() {
                    log::debug!("ws send failed, connection closed");
                    break;
                }
            }
        });

        // 接收任务: 从 WS 读取, 解析后塞入 evt_tx
        tokio::spawn(async move {
            while let Some(msg) = ws_rx.next().await {
                let msg = match msg {
                    Ok(m) => m,
                    Err(e) => {
                        log::warn!("ws recv error: {e}");
                        break;
                    }
                };
                if let Message::Text(text) = msg {
                    match ServerEvent::from_json(&text) {
                        Ok(evt) => {
                            if evt_tx.send(evt).is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            log::debug!("invalid server event: {e}");
                        }
                    }
                }
            }
        });

        Ok(Self {
            tx: cmd_tx,
            rx: Arc::new(TokioMutex::new(evt_rx)),
        })
    }

    /// 发送客户端命令(不阻塞)
    pub fn send(&self, msg: ClientMessage) {
        if let Err(e) = self.tx.send(msg) {
            log::debug!("send command failed: {e}");
        }
    }

    /// 尝试接收一个事件(非阻塞)
    pub async fn try_recv(&self) -> Option<ServerEvent> {
        let mut rx = self.rx.lock().await;
        rx.try_recv().ok()
    }

    /// 排空所有可用事件
    pub async fn drain(&self) -> Vec<ServerEvent> {
        let mut rx = self.rx.lock().await;
        let mut out = Vec::new();
        while let Ok(evt) = rx.try_recv() {
            out.push(evt);
        }
        out
    }
}

// 避免未使用警告
#[allow(dead_code)]
type _Stream = MaybeTlsStream<tokio::net::TcpStream>;
