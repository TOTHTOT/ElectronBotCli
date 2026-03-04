//! 视频模块 - 类型定义

use std::sync::{Arc, Mutex};

/// 帧数据 - 区分是否已经是 JPEG 编码
#[derive(Debug, Clone)]
pub enum FrameData {
    /// 已经是 JPEG 编码的数据，浏览器可直接显示
    Jpeg(Vec<u8>),
    /// 原始 BGR 数据，用于图像识别等后续处理
    RawBgr(Vec<u8>),
}

impl FrameData {
    /// 获取 JPEG 数据（如果已经是 JPEG 则直接返回，否则返回 None）
    pub fn as_jpeg(&self) -> Option<&Vec<u8>> {
        match self {
            FrameData::Jpeg(data) => Some(data),
            FrameData::RawBgr(_) => None,
        }
    }

    /// 获取原始 BGR 数据
    pub fn as_raw_bgr(&self) -> Option<&Vec<u8>> {
        match self {
            FrameData::Jpeg(_) => None,
            FrameData::RawBgr(data) => Some(data),
        }
    }

    /// 判断是否已经是 JPEG
    #[allow(dead_code)]
    pub fn is_jpeg(&self) -> bool {
        matches!(self, FrameData::Jpeg(_))
    }
}

/// 帧缓存类型 - 存储 FrameData 而非原始 Vec
pub type FrameCache = Arc<Mutex<Option<FrameData>>>;
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
