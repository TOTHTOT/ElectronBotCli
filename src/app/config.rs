use crate::media::video::process::RotateAngle;
use cfg_if::cfg_if;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// 应用配置
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    pub speaker_name: String,
    pub microphone_name: String,
    pub camera_index: String,
    pub rotation: RotateAngle,
    pub wifi_ssid: String,
    pub wifi_password: String,
    /// 在线 LLM API 地址
    pub llm_api_base: String,
    /// 在线 LLM API Key
    pub llm_api_key: String,
    /// 在线 LLM 模型名称
    pub llm_model: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            speaker_name: {
                cfg_if! {
                    if #[cfg(target_os = "linux")] {
                        "sysdefault:CARD=CODEC".to_string()
                    } else if #[cfg(target_os = "macos")] {
                        "BuiltInSpeakerDevice".to_string()
                    }
                }
            },
            microphone_name: "麦克风阵列".to_string(),
            rotation: RotateAngle::Rotate270,
            camera_index: "0".to_string(),
            wifi_ssid: "".to_string(),
            wifi_password: "".to_string(),
            llm_api_base: "https://ark.cn-beijing.volces.com/api/v3".to_string(),
            llm_api_key: "6804a808-871b-4d70-8d21-51fdfb49cd4b".to_string(),
            llm_model: "doubao-seed-1-6-251015".to_string(),
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
        self.speaker_name = name;
        let _ = self.save();
    }

    pub fn set_wifi(&mut self, ssid: String, password: String) {
        self.wifi_ssid = ssid;
        self.wifi_password = password;
        let _ = self.save();
    }
}
