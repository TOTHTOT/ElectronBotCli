/// CDC 设备连接状态
#[derive(Debug, Clone, PartialEq)]
pub enum DeviceState {
    Disconnected,
    Connected(String),
    Error(String),
}

/// 各个关节的运动角度
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct JointConfig {
    pub enable: u8,       // 1: 使能, 0: 掉电
    pub angles: [f32; 6], // 6个关节的角度
    pub padding: [u8; 7], // 补齐到 32 字节
}

impl Default for JointConfig {
    fn default() -> Self {
        Self {
            enable: 0,
            angles: [0.0; 6],
            padding: [0u8; 7],
        }
    }
}

impl JointConfig {
    pub fn to_bytes(self) -> [u8; 32] {
        let mut bytes = [0u8; 32];
        bytes[0] = self.enable;
        for i in 0..6 {
            let b = self.angles[i].to_le_bytes();
            bytes[1 + i * 4..1 + i * 4 + 4].copy_from_slice(&b);
        }
        bytes
    }
}
pub const BUFFER_COUNT: usize = 2;
