use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// 应用配置
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    pub speech_name: String,
    pub wifi_ssid: String,
    pub wifi_password: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            speech_name: "麦克风阵列".to_string(),
            wifi_ssid: "".to_string(),
            wifi_password: "".to_string(),
        }
    }
}

#[allow(dead_code)]
impl AppConfig {
    /// 配置文件路径
    const CONFIG_PATH: &'static str = "config.toml";

    /// 加载配置
    ///
    /// 如果配置文件不存在或解析失败，返回默认配置
    pub fn load() -> Self {
        match fs::read_to_string(Self::CONFIG_PATH) {
            Ok(content) => toml::from_str(&content).unwrap_or_else(|e| {
                log::warn!("Failed to parse config: {e}, using default");
                Self::default()
            }),
            Err(e) => {
                log::info!("Config file not found: {e}, using default");
                let config = Self::default();
                // 保存默认配置
                if let Err(e) = config.save() {
                    log::warn!("Failed to save default config: {e}");
                }
                config
            }
        }
    }

    /// 保存配置
    pub fn save(&self) -> anyhow::Result<()> {
        let content = toml::to_string_pretty(self)?;
        fs::write(Path::new(Self::CONFIG_PATH), content)?;
        log::info!("Config saved to {}", Self::CONFIG_PATH);
        Ok(())
    }

    /// 更新麦克风配置并保存
    pub fn set_speech_name(&mut self, name: String) {
        self.speech_name = name;
        let _ = self.save();
    }

    /// 更新 WiFi 配置并保存
    pub fn set_wifi(&mut self, ssid: String, password: String) {
        self.wifi_ssid = ssid;
        self.wifi_password = password;
        let _ = self.save();
    }
}
