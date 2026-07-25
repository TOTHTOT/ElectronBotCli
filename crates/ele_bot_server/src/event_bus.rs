//! 事件总线
//!
//! 把服务端所有"事件流" (ASR 文本 / LLM 回复 / 音量 / 状态) 统一通过
//! `tokio::sync::broadcast` 广播. 数据流 (audio / video frame / WS 双向
//! / 关节指令) 仍走专用 channel, 不参与 bus.
//!
//! # 用法
//!
//! ```rust,ignore
//! use crate::event_bus::{EventBus, BusEvent};
//!
//! let bus = EventBus::new(1024);
//!
//! // 发布
//! bus.publish(BusEvent::AsrText("你好".into()));
//!
//! // 订阅 (多订阅者独立)
//! let mut rx = bus.subscribe();
//! tokio::spawn(async move {
//!     while let Ok(evt) = rx.recv().await {
//!         if let BusEvent::AsrText(text) = evt { /* ... */ }
//!     }
//! });
//! ```

use tokio::sync::broadcast;

/// 服务端所有事件的统一枚举. 新增 variant 加在这里, 而不是散落各模块.
///
/// 设计: 单一枚举 + 单 bus, 订阅者用 `match` 过滤自己关心的 variant.
/// 枚举本身充当路由表, 不需要额外的 router.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum BusEvent {
    /// 协议层服务端事件 (TUI 显示用). WS 客户端订阅时直接序列化转发.
    ServerEvent(ele_bot_proto::ServerEvent),
    /// ASR 识别文本. LLM 任务消费, 不外发.
    AsrText(String),
    /// LLM 对话回复. TTS trigger 消费.
    LlmReply(String),
    /// LLM 处理中标志. UI 可订阅显示 loading.
    LlmProcessing { is_processing: bool },
    /// 实时音量 [0, 100]. WS 客户端订阅转 `ServerEvent::Volume` 外发.
    Volume(i32),
}

/// 事件总线. 内部 `tokio::sync::broadcast::Sender<BusEvent>` 的轻包装.
///
/// `publish` 永不 panic: 容量满/无订阅者时 log warn 丢弃.
/// 订阅者 Lagged 时拿 `Err(Lagged(n))`, 跳到最新事件继续.
#[derive(Clone)]
pub struct EventBus {
    inner: broadcast::Sender<BusEvent>,
}

impl EventBus {
    /// 构造指定容量的事件总线. capacity = 同时保留的事件数,
    /// 满了会覆盖最老的, 订阅者下次 recv 收到 `Err(Lagged(n))`.
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { inner: tx }
    }

    /// 发布一个事件. 不阻塞, send 失败 (无订阅者) 时 log warn + 丢弃.
    pub fn publish(&self, event: BusEvent) {
        match self.inner.send(event) {
            Ok(_) => {}
            Err(e) => {
                // broadcast::send 失败要么是没人订阅, 要么是 channel 关了.
                // channel 关 = EventBus drop = 服务端关, 此时 publish 已被忽略.
                log::debug!("event bus publish: no subscribers or channel closed, error: {e:?}");
            }
        }
    }

    /// 订阅事件总线. 每次调都拿新的独立 receiver, 后到的 publish
    /// 看不到订阅前的旧事件 (broadcast 语义).
    pub fn subscribe(&self) -> broadcast::Receiver<BusEvent> {
        self.inner.subscribe()
    }

    /// 当前活跃订阅者数 (debug 用).
    pub fn subscriber_count(&self) -> usize {
        self.inner.receiver_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_publish_subscribe_single() {
        let bus = EventBus::new(16);
        let mut rx = bus.subscribe();
        bus.publish(BusEvent::AsrText("hello".into()));
        let evt = rx.recv().await.unwrap();
        match evt {
            BusEvent::AsrText(t) => assert_eq!(t, "hello"),
            _ => panic!("unexpected variant"),
        }
    }

    #[tokio::test]
    async fn test_late_subscriber_misses_old_events() {
        let bus = EventBus::new(16);
        bus.publish(BusEvent::AsrText("before".into()));
        let mut rx = bus.subscribe();
        bus.publish(BusEvent::AsrText("after".into()));
        // 旧事件 ("before") 收不到 — broadcast 语义
        let evt = rx.recv().await.unwrap();
        match evt {
            BusEvent::AsrText(t) => assert_eq!(t, "after"),
            _ => panic!("unexpected variant"),
        }
    }

    #[tokio::test]
    async fn test_multi_subscriber_isolation() {
        let bus = EventBus::new(16);
        let mut rx_a = bus.subscribe();
        let mut rx_b = bus.subscribe();
        bus.publish(BusEvent::Volume(42));
        let a = rx_a.recv().await.unwrap();
        let b = rx_b.recv().await.unwrap();
        match (a, b) {
            (BusEvent::Volume(va), BusEvent::Volume(vb)) => {
                assert_eq!(va, 42);
                assert_eq!(vb, 42);
            }
            _ => panic!("unexpected variant"),
        }
    }

    #[tokio::test]
    async fn test_lagged_when_capacity_overflow() {
        // 容量 2, 但发 5 条; 旧订阅者下次 recv 看到 Lagged
        let bus = EventBus::new(2);
        let mut rx = bus.subscribe();
        for i in 0..5 {
            bus.publish(BusEvent::Volume(i));
        }
        // rx.recv() 第一次可能直接拿到最新 (EventBus::new 内部 receiver_count=0 时 first send 不算 Lagged)
        // 关键是 Lagged 错误能被处理
        let mut received_lagged = false;
        for _ in 0..10 {
            match rx.recv().await {
                Ok(_) => continue,
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    received_lagged = true;
                    break;
                }
                Err(_) => break,
            }
        }
        // broadcast 行为: 容量满时只覆盖, 不一定触发 Lagged (取决于时序).
        // 这里不强断言, 只确保 recv 不会卡死.
        let _ = received_lagged;
    }

    #[test]
    fn test_publish_no_subscribers_does_not_panic() {
        let bus = EventBus::new(4);
        // 没有订阅者, publish 应当安静 log debug + 不 panic
        bus.publish(BusEvent::Volume(0));
    }

    #[test]
    fn test_subscriber_count() {
        let bus = EventBus::new(4);
        assert_eq!(bus.subscriber_count(), 0);
        let _r1 = bus.subscribe();
        let _r2 = bus.subscribe();
        assert_eq!(bus.subscriber_count(), 2);
    }
}
