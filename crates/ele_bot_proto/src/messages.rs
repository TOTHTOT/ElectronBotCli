//! WebSocket 消息
//!
//! 定义客户端→服务端(ClientMessage)和服务端→客户端(ServerEvent)的消息。
//! 协议基于 JSON, 消息体序列化为单一字符串。

use crate::types::{
    AppConfig, CameraInfoDto, DeviceInfoDto, DisplayMode, FacePosition, JointConfig, JointState,
    LlmResponse, Mood, SystemStatsDto, SERVO_COUNT,
};
use serde::{Deserialize, Serialize};

/// 客户端发送的命令
// `AppConfig` 体积较大, 但作为 WS 消息的单一载荷, boxing 会让序列化/反序列化
// 多一层 Box 解包开销且无实质收益; 这里用 allow 跳过该 lint.
#[allow(clippy::large_enum_variant)]
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
    /// 请求所有音频输入设备 (服务端响应 `ServerEvent::InputDevices`)
    ListInputDevices,
    /// 请求所有音频输出设备 (服务端响应 `ServerEvent::OutputDevices`)
    ListOutputDevices,
    /// 请求所有摄像头 (服务端响应 `ServerEvent::Cameras`)
    ListCameras,
    /// 清空 LLM 对话历史与个人记忆 (服务端响应 `ServerEvent::LlmMemoryCleared`
    /// 或 `ServerEvent::Error`); 历史/记忆由 zeroclaw 托管, 此命令为整体清空入口
    ClearLlmMemory,
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
// 同 `ClientMessage`: `AppConfig` 体积大但作为单字段载荷 boxing 无收益.
#[allow(clippy::large_enum_variant)]
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
    /// 当前麦克风输入音量 (0..=100), 由 dB 对数刻度从 cpal 峰值样本映射得出
    Volume { value: i32 },
    /// 服务端枚举到的所有音频输入设备 (响应 `ListInputDevices`)
    InputDevices { devices: Vec<DeviceInfoDto> },
    /// 服务端枚举到的所有音频输出设备 (响应 `ListOutputDevices`)
    OutputDevices { devices: Vec<DeviceInfoDto> },
    /// 服务端枚举到的所有摄像头 (响应 `ListCameras`).
    /// 列表为空时表示当前没有任何可用摄像头, 不代表错误.
    Cameras { cameras: Vec<CameraInfoDto> },
    /// 系统状态 (SoC 温度 / CPU / 内存), 服务端定时推送
    SystemStats { stats: SystemStatsDto },
    /// LLM 对话历史与个人记忆已清空 (响应 `ClearLlmMemory`)
    LlmMemoryCleared,
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

    #[test]
    fn server_event_volume_roundtrip() {
        let evt = ServerEvent::Volume { value: 42 };
        let json = evt.to_json().unwrap();
        assert!(json.contains("\"type\":\"volume\""));
        let parsed = ServerEvent::from_json(&json).unwrap();
        match parsed {
            ServerEvent::Volume { value } => assert_eq!(value, 42),
            _ => panic!("expected Volume event"),
        }
    }

    #[test]
    fn list_devices_request_roundtrip() {
        let msg = ClientMessage::ListInputDevices;
        let json = msg.to_json().unwrap();
        assert!(json.contains("\"type\":\"list_input_devices\""));
        let parsed = ClientMessage::from_json(&json).unwrap();
        assert!(matches!(parsed, ClientMessage::ListInputDevices));

        let msg = ClientMessage::ListOutputDevices;
        let json = msg.to_json().unwrap();
        let parsed = ClientMessage::from_json(&json).unwrap();
        assert!(matches!(parsed, ClientMessage::ListOutputDevices));

        let msg = ClientMessage::ListCameras;
        let json = msg.to_json().unwrap();
        assert!(json.contains("\"type\":\"list_cameras\""));
        let parsed = ClientMessage::from_json(&json).unwrap();
        assert!(matches!(parsed, ClientMessage::ListCameras));
    }

    #[test]
    fn clear_llm_memory_roundtrip() {
        let msg = ClientMessage::ClearLlmMemory;
        let json = msg.to_json().unwrap();
        assert!(json.contains("\"type\":\"clear_llm_memory\""));
        let parsed = ClientMessage::from_json(&json).unwrap();
        assert!(matches!(parsed, ClientMessage::ClearLlmMemory));

        let evt = ServerEvent::LlmMemoryCleared;
        let json = evt.to_json().unwrap();
        assert!(json.contains("\"type\":\"llm_memory_cleared\""));
        let parsed = ServerEvent::from_json(&json).unwrap();
        assert!(matches!(parsed, ServerEvent::LlmMemoryCleared));
    }

    #[test]
    fn input_devices_event_roundtrip() {
        let evt = ServerEvent::InputDevices {
            devices: vec![DeviceInfoDto {
                id: "{0.0.0.00000000}.{test-guid}".to_string(),
                name: "麦克风阵列".to_string(),
                display: "WASAPI 麦克风阵列 (2ch, 48000Hz)".to_string(),
                driver: Some("WASAPI".to_string()),
                channels: 2,
                sample_rate: 48000,
            }],
        };
        let json = evt.to_json().unwrap();
        assert!(json.contains("\"type\":\"input_devices\""));
        let parsed = ServerEvent::from_json(&json).unwrap();
        match parsed {
            ServerEvent::InputDevices { devices } => {
                assert_eq!(devices.len(), 1);
                assert_eq!(devices[0].name, "麦克风阵列");
                assert_eq!(devices[0].channels, 2);
                assert_eq!(devices[0].driver.as_deref(), Some("WASAPI"));
            }
            _ => panic!("expected InputDevices event"),
        }
    }

    #[test]
    fn output_devices_event_roundtrip() {
        let evt = ServerEvent::OutputDevices {
            devices: vec![DeviceInfoDto::default()],
        };
        let json = evt.to_json().unwrap();
        assert!(json.contains("\"type\":\"output_devices\""));
        let parsed = ServerEvent::from_json(&json).unwrap();
        assert!(matches!(parsed, ServerEvent::OutputDevices { .. }));
    }

    #[test]
    fn system_stats_roundtrip() {
        let evt = ServerEvent::SystemStats {
            stats: SystemStatsDto {
                soc_temp_c: Some(52.3),
                cpu_usage: 17.5,
                mem_used_mb: 812,
                mem_total_mb: 2048,
            },
        };
        let json = evt.to_json().unwrap();
        assert!(json.contains("\"type\":\"system_stats\""));
        let parsed = ServerEvent::from_json(&json).unwrap();
        match parsed {
            ServerEvent::SystemStats { stats } => {
                assert_eq!(stats.soc_temp_c, Some(52.3));
                assert_eq!(stats.mem_total_mb, 2048);
            }
            _ => panic!("expected SystemStats event"),
        }
        // 温度缺失的平台 (非 Linux) 也应正常往返
        let evt = ServerEvent::SystemStats {
            stats: SystemStatsDto {
                soc_temp_c: None,
                ..Default::default()
            },
        };
        let json = evt.to_json().unwrap();
        assert!(matches!(
            ServerEvent::from_json(&json).unwrap(),
            ServerEvent::SystemStats { .. }
        ));
    }

    #[test]
    fn cameras_event_roundtrip() {
        let evt = ServerEvent::Cameras {
            cameras: vec![CameraInfoDto {
                id: "0".to_string(),
                name: "Integrated Camera".to_string(),
                display: "Integrated Camera (id=0, USB)".to_string(),
            }],
        };
        let json = evt.to_json().unwrap();
        assert!(json.contains("\"type\":\"cameras\""));
        let parsed = ServerEvent::from_json(&json).unwrap();
        match parsed {
            ServerEvent::Cameras { cameras } => {
                assert_eq!(cameras.len(), 1);
                assert_eq!(cameras[0].id, "0");
                assert_eq!(cameras[0].display, "Integrated Camera (id=0, USB)");
            }
            _ => panic!("expected Cameras event"),
        }
    }

    #[test]
    fn cameras_event_empty_is_ok() {
        // 没有任何摄像头时, 服务端仍回 Cameras { cameras: vec![] },
        // 客户端 picker 不应报错.
        let evt = ServerEvent::Cameras { cameras: vec![] };
        let json = evt.to_json().unwrap();
        let parsed = ServerEvent::from_json(&json).unwrap();
        match parsed {
            ServerEvent::Cameras { cameras } => assert!(cameras.is_empty()),
            _ => panic!("expected Cameras event"),
        }
    }
}
