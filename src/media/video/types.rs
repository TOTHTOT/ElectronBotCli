//! 视频模块 - 类型定义

use crate::vision::face::detector::FaceDetectionResult;
use bytes::Bytes;
use tokio::sync::broadcast;

/// 帧数据 - 使用 Bytes 避免内存复制
#[derive(Debug, Clone)]
pub enum FrameData {
    /// 已经是 JPEG 编码的数据，浏览器可直接显示
    #[allow(dead_code)]
    Jpeg(Bytes),
    /// 原始 RGB 数据，用于图像识别等后续处理
    RawRgb(Bytes),
}

impl FrameData {
    /// 获取 JPEG 数据
    pub fn as_jpeg(&self) -> Option<&Bytes> {
        match self {
            FrameData::Jpeg(data) => Some(data),
            FrameData::RawRgb(_) => None,
        }
    }

    /// 获取原始 BGR 数据
    pub fn as_raw_rgb(&self) -> Option<&Bytes> {
        match self {
            FrameData::Jpeg(_) => None,
            FrameData::RawRgb(data) => Some(data),
        }
    }

    /// 判断是否已经是 JPEG
    #[allow(dead_code)]
    pub fn is_jpeg(&self) -> bool {
        matches!(self, FrameData::Jpeg(_))
    }
}

/// 一帧图像数据包含的内容
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct FrameInfo {
    pub frame_data: FrameData,          // 原始一帧数据, 以及画过框的
    pub face_info: FaceDetectionResult, // 脸部信息, 原始宽坐标
    pub focused: bool,                  // 是否正在被注释
    pub emotion: boteyes::Mood,         // 当前情绪
}

/// 帧缓存类型 - 使用 tokio broadcast 通道实现事件驱动
/// Sender 用于发送帧，Receiver 用于接收帧
pub type FrameCache = broadcast::Sender<FrameInfo>;

/// 摄像头支持的格式和分辨率
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct CameraFormat {
    /// 宽度
    pub width: u32,
    /// 高度
    pub height: u32,
    /// 帧率
    pub fps: u32,
    /// 格式描述
    pub format_desc: String,
}

impl std::fmt::Display for CameraFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}x{} @{}fps [{}]",
            self.width, self.height, self.fps, self.format_desc
        )
    }
}
