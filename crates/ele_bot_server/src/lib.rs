//! `ElectronBotCli` 服务端库
//!
//! 持有所有硬件资源(机器人、摄像头、麦克风、LLM 推理等),
//! 通过 WebSocket 与客户端通信。

pub mod event_bus;
pub mod face_tracker;
pub mod llm;
pub mod media;
pub mod model_manager;
pub mod robot;
pub mod state;
pub mod sysmon;
pub mod test_mode;
pub mod vision;
pub mod web;
pub mod ws;
