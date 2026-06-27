//! 共享数据类型
//!
//! 服务端与客户端之间传输的所有数据类型都在此定义。
//! 注意:这里定义的类型独立于第三方库(boteyes/electron_bot 等),
//! 在 server/client 边界做转换。

use serde::{Deserialize, Serialize};

/// 情感状态
///
/// 与 boteyes::Mood 一一对应, 通过 `From` 互转。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mood {
    #[default]
    Default,
    Happy,
    Sad,
    Angry,
    Surprise,
    Confuse,
    Loading,
}

impl Mood {
    pub fn as_str(&self) -> &'static str {
        match self {
            Mood::Default => "default",
            Mood::Happy => "happy",
            Mood::Sad => "sad",
            Mood::Angry => "angry",
            Mood::Surprise => "surprise",
            Mood::Confuse => "confuse",
            Mood::Loading => "loading",
        }
    }
}

/// 舵机数量
pub const SERVO_COUNT: usize = 6;

/// 舵机配置(用于通过 USB 发送给机器人)
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct JointConfig {
    /// 使能标志
    pub enable: u8,
    /// 6 个舵机角度
    pub angles: [f32; SERVO_COUNT],
}

impl Default for JointConfig {
    fn default() -> Self {
        Self {
            enable: 0,
            angles: [0.0; SERVO_COUNT],
        }
    }
}

/// 舵机值状态(用于 UI 显示)
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct JointState {
    /// 6 个舵机当前显示值
    pub values: [i16; SERVO_COUNT],
    /// 当前选中的舵机索引
    pub selected: usize,
}

/// 单个舵机动作
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Action {
    pub servo_index: u8,
    pub angle: i16,
    pub duration_ms: u32,
}

impl Default for Action {
    fn default() -> Self {
        Self {
            servo_index: 0,
            angle: 0,
            duration_ms: 300,
        }
    }
}

/// LLM 响应
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LlmResponse {
    pub mood: Mood,
    pub actions: Vec<Action>,
}

/// 摄像头旋转角度
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RotateAngle {
    #[default]
    Rotate0,
    Rotate90,
    Rotate180,
    Rotate270,
}

/// LCD 显示模式
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayMode {
    Static,
    #[default]
    Eyes,
    TestPattern,
}

/// 应用配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub speech_name: String,
    pub camera_index: String,
    pub rotation: RotateAngle,
    pub wifi_ssid: String,
    pub wifi_password: String,
    /// 输出设备名称（空字符串表示使用系统默认设备）
    pub output_device: String,
    pub llm_api_base: String,
    pub llm_api_key: String,
    pub llm_model: String,
    pub tts_enabled: bool,
    pub tts_speed: f32,
    pub tts_voice: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            speech_name: "麦克风阵列".to_string(),
            rotation: RotateAngle::Rotate270,
            camera_index: "0".to_string(),
            wifi_ssid: String::new(),
            wifi_password: String::new(),
            output_device: String::new(),
            llm_api_base: String::new(),
            llm_api_key: String::new(),
            llm_model: "doubao-seed-1-6-251015".to_string(),
            tts_enabled: true,
            tts_speed: 1.0,
            tts_voice: "af_sarah".to_string(),
        }
    }
}

/// 摄像头分辨率
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct CameraResolution {
    pub width: u32,
    pub height: u32,
}

/// 人脸位置(用于追踪)
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct FacePosition {
    pub x: f32,
    pub has_face: bool,
}

impl AppConfig {
    pub const CONFIG_PATH: &'static str = "config.toml";

    /// 从文件加载, 失败则使用默认值
    pub fn load_or_default() -> Self {
        match std::fs::read_to_string(Self::CONFIG_PATH) {
            Ok(content) => toml::from_str::<Self>(&content).unwrap_or_else(|e| {
                log::warn!("Failed to parse config: {e}, using default");
                Self::default()
            }),
            Err(e) => {
                log::warn!("Config file not found: {e}, using default");
                Self::default()
            }
        }
    }

    /// 持久化到文件
    pub fn save(&self) -> anyhow::Result<()> {
        let content = toml::to_string_pretty(self)?;
        std::fs::write(std::path::Path::new(Self::CONFIG_PATH), content)?;
        Ok(())
    }
}
