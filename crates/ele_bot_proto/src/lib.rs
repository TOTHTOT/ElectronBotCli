//! 共享协议层
//!
//! 定义服务端与客户端之间的所有消息与共享数据类型。
//! 纯数据层，无业务逻辑,服务端和客户端都依赖此 crate。

pub mod messages;
pub mod types;

pub use messages::*;
pub use types::*;
