use crate::media::video::process::RotateAngle;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// 应用配置
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    pub speech_name: String,
    pub camera_index: String,
    pub rotation: RotateAngle,
    pub wifi_ssid: String,
    pub wifi_password: String,
    /// 输出设备名称（空字符串表示使用系统默认设备）
    pub output_device: String,
    /// 在线 LLM API 地址
    pub llm_api_base: String,
    /// 在线 LLM API Key
    pub llm_api_key: String,
    /// 在线 LLM 模型名称
    pub llm_model: String,
    /// TTS 是否启用
    pub tts_enabled: bool,
    /// TTS 语速
    pub tts_speed: f32,
    /// TTS 语音
    pub tts_voice: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            speech_name: "麦克风阵列".to_string(),
            rotation: RotateAngle::Rotate270,
            camera_index: "0".to_string(),
            wifi_ssid: "".to_string(),
            wifi_password: "".to_string(),
            output_device: "".to_string(),
            llm_api_base: "".to_string(),
            llm_api_key: "".to_string(),
            llm_model: "doubao-seed-1-6-251015".to_string(),
            tts_enabled: true,
            tts_speed: 1.0,
            tts_voice: "af_sarah".to_string(),
        }
    }
}

#[allow(dead_code)]
impl AppConfig {
    const CONFIG_PATH: &'static str = "config.toml";

    pub fn load() -> Self {
        let (config, needs_save) = match fs::read_to_string(Self::CONFIG_PATH) {
            Ok(content) => match toml::from_str::<Self>(&content) {
                Ok(config) => (config, false),
                Err(e) => {
                    log::warn!("Failed to parse config: {e}, using default");
                    (Self::default(), true)
                }
            },
            Err(e) => {
                log::warn!("Config file not found: {e}, using default");
                (Self::default(), true)
            }
        };

        if needs_save {
            let _ = config.save();
        }
        config
    }
    pub fn save(&self) -> anyhow::Result<()> {
        let content = toml::to_string_pretty(self)?;
        fs::write(Path::new(Self::CONFIG_PATH), content)?;
        log::info!("Config saved to {}", Self::CONFIG_PATH);
        Ok(())
    }

    pub fn set_speech_name(&mut self, name: String) {
        self.speech_name = name;
        let _ = self.save();
    }

    pub fn set_output_device(&mut self, name: String) {
        self.output_device = name;
        let _ = self.save();
    }

    pub fn set_wifi(&mut self, ssid: String, password: String) {
        self.wifi_ssid = ssid;
        self.wifi_password = password;
        let _ = self.save();
    }
}
