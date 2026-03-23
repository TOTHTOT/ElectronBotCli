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
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            speech_name: "麦克风阵列".to_string(),
            rotation: RotateAngle::Rotate270,
            camera_index: "0".to_string(),
            wifi_ssid: "".to_string(),
            wifi_password: "".to_string(),
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

    pub fn set_wifi(&mut self, ssid: String, password: String) {
        self.wifi_ssid = ssid;
        self.wifi_password = password;
        let _ = self.save();
    }
}
