//! 共享数据类型
//!
//! 服务端与客户端之间传输的所有数据类型都在此定义。
//! `注意:这里定义的类型独立于第三方库(boteyes/electron_bot` 等),
//! 在 server/client 边界做转换。

use serde::{Deserialize, Serialize};

/// 情感状态
///
/// 与 `boteyes::Mood` 一一对应, 通过 `From` 互转。
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
    #[must_use]
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
    /// LLM 生成的对用户回复文本 (走 TTS 播报). 旧客户端忽略 None 字段.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_text: Option<String>,
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
    /// 麦克风设备稳定标识 (cpal `DeviceId`); 与 `speech_name` 二选一优先
    pub speech_device_id: Option<String>,
    pub camera_index: String,
    pub rotation: RotateAngle,
    pub wifi_ssid: String,
    pub wifi_password: String,
    /// 输出设备名称（空字符串表示使用系统默认设备）
    pub output_device: String,
    /// 输出设备稳定标识 (cpal `DeviceId`); 与 `output_device` 二选一优先
    pub output_device_id: Option<String>,
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
            speech_device_id: None,
            rotation: RotateAngle::Rotate270,
            camera_index: "0".to_string(),
            wifi_ssid: String::new(),
            wifi_password: String::new(),
            output_device: String::new(),
            output_device_id: None,
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

/// 音频设备信息(通过 `ListInputDevices` / `ListOutputDevices` 传输)
///
/// `id` 是 cpal `Device::id()` 序列化的稳定标识符 (Windows 上是 `IMMDevice`
/// endpoint ID 字符串, Linux 是 ALSA path, macOS 是 UID). 同一 OS 会话内
/// 唯一, 跨枚举顺序变化稳定, 用于服务端按设备匹配 — 写入
/// `AppConfig.speech_device_id` / `output_device_id` 时必须用此字段.
///
/// `name` 是 cpal 的精确设备名 (Windows 上是 `FriendlyName`, 多 endpoint /
/// 多虚拟设备常重名), 仅作为 `id` 失效时的兜底匹配键以及向后兼容老 config.
///
/// `display` 是给人类看的拼接字符串, 客户端 MUST NOT 用正则解析它.
///
/// `driver` 独立成字段而非藏在 `display` 字符串里, 便于客户端按需布局 /
/// 着色; 见 `enhance-device-picker` spec 中的 "设备显示须呈现驱动字段" 需求.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfoDto {
    /// 稳定设备标识 (cpal `DeviceId` 序列化), 服务端按此匹配
    pub id: String,
    /// 精确设备名 (exact match key, 仅作为 id 兜底)
    pub name: String,
    /// 给人类看的拼接串 (e.g. "WASAPI 麦克风阵列 (2ch, 48000Hz)")
    pub display: String,
    /// 后端驱动名 (e.g. "WASAPI" / "MME" / "ALSA"), cpal 不可用时为 `None`
    pub driver: Option<String>,
    /// 输入/输出通道数, 不可用时为 0
    pub channels: u16,
    /// 默认采样率, 不可用时为 0
    pub sample_rate: u32,
}

/// 人脸位置(用于追踪)
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct FacePosition {
    pub x: f32,
    pub has_face: bool,
}

/// 摄像头信息(通过 `ListCameras` / `Cameras` 传输)
///
/// `id` 是 nokhwa 摄像头枚举结果的稳定标识: 当 `AppConfig.camera_index`
/// 能被解析为整数时, 服务端把它映射成 `nokhwa::CameraIndex::Index`,
/// 此时 `id` 取 `index.to_string()`; 当配置里是 USB path / 设备描述字符串
/// 时, 服务端用 `CameraIndex::String`, 此时 `id` 与配置字符串相等.
/// 客户端持久化 picker 选择时应直接把 `id` 写入 `AppConfig.camera_index`.
///
/// `name` 是 nokhwa `CameraInfo::human_readable_name`(或 `description`)
/// 兜底用, 不参与匹配.
///
/// `display` 是给人类看的拼接字符串, 客户端 MUST NOT 用正则解析.
///
/// 字段命名刻意比 `DeviceInfoDto` 简单, 摄像头不像 cpal 设备那样有 driver/
/// 通道数/采样率维度.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CameraInfoDto {
    /// 稳定摄像头标识 (nokhwa `CameraInfo.index` 序列化或路径字符串),
    /// 服务端按此匹配, 与 `AppConfig.camera_index` 一一对应.
    pub id: String,
    /// 精确摄像头名 (match 兜底).
    pub name: String,
    /// 给人类看的拼接串 (e.g. "Integrated Camera (id=0, USB)").
    pub display: String,
}

impl AppConfig {
    pub const CONFIG_PATH: &'static str = "config.toml";

    /// 从文件加载, 失败则使用默认值
    #[must_use]
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
