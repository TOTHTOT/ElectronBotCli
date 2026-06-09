//! WebSocket 消息
//!
//! 定义客户端→服务端(ClientMessage)和服务端→客户端(ServerEvent)的消息。
//! 协议基于 JSON, 消息体序列化为单一字符串。

use crate::types::*;
use serde::{Deserialize, Serialize};

/// 客户端发送的命令
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    /// 心跳
    Ping,
    /// 请求当前配置
    GetConfig,
    /// 设置配置
    SetConfig { config: AppConfig },
    /// 连接机器人
    ConnectRobot,
    /// 断开机器人连接
    DisconnectRobot,
    /// 设置单个舵机角度
    SetJoint { servo_index: u8, angle: f32 },
    /// 设置所有舵机角度
    SetJoints { angles: [f32; SERVO_COUNT] },
    /// 选择舵机索引(下一个/上一个)
    SelectServo { delta: i8 },
    /// 舵机角度增加/减少一格
    AdjustSelectedServo { delta: i16 },
    /// 设置眼睛表情
    SetMood { mood: Mood },
    /// 设置 LCD 显示模式
    SetLcdMode { mode: DisplayMode },
    /// 切换人脸追踪
    SetFaceTracking { enabled: bool },
    /// 发送文本给 LLM 分析
    SendLlmText { text: String },
    /// TTS 播放文本
    TtsSpeak {
        text: String,
        speed: f32,
        streaming: bool,
    },
    /// 截图
    TakeScreenshot,
}

impl ClientMessage {
    /// JSON 序列化为字符串(用于 WebSocket 文本帧)
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }

    /// 从 JSON 字符串解析
    pub fn from_json(s: &str) -> serde_json::Result<Self> {
        serde_json::from_str(s)
    }
}

/// 服务端推送的事件
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerEvent {
    /// 心跳响应
    Pong,
    /// 初始配置(连接成功后推送一次)
    Config { config: AppConfig },
    /// 连接状态变化
    Connection { is_connected: bool },
    /// 舵机状态更新
    JointState { state: JointState },
    /// 舵机配置更新(即将发送给 USB)
    JointConfig { config: JointConfig },
    /// 情绪更新
    Mood { mood: Mood },
    /// LLM 响应
    LlmResponse { response: LlmResponse },
    /// LLM 处理状态
    LlmProcessing { is_processing: bool },
    /// 截图保存结果
    ScreenshotSaved { path: String },
    /// 错误
    Error { message: String },
    /// 人脸位置(用于客户端可选的预览显示)
    Face { position: FacePosition },
    /// 摄像头分辨率(初始化时推送)
    CameraResolution { width: u32, height: u32 },
}

impl ServerEvent {
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string(self)
    }

    pub fn from_json(s: &str) -> serde_json::Result<Self> {
        serde_json::from_str(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ping_pong_roundtrip() {
        let msg = ClientMessage::Ping;
        let json = msg.to_json().unwrap();
        let parsed = ClientMessage::from_json(&json).unwrap();
        assert!(matches!(parsed, ClientMessage::Ping));
    }

    #[test]
    fn set_joint_serialize() {
        let msg = ClientMessage::SetJoint {
            servo_index: 0,
            angle: 45.0,
        };
        let json = msg.to_json().unwrap();
        assert!(json.contains("\"type\":\"set_joint\""));
        let parsed = ClientMessage::from_json(&json).unwrap();
        assert!(matches!(parsed, ClientMessage::SetJoint { .. }));
    }

    #[test]
    fn server_event_mood() {
        let evt = ServerEvent::Mood { mood: Mood::Happy };
        let json = evt.to_json().unwrap();
        let parsed = ServerEvent::from_json(&json).unwrap();
        match parsed {
            ServerEvent::Mood { mood } => assert_eq!(mood, Mood::Happy),
            _ => panic!("expected Mood event"),
        }
    }
}
