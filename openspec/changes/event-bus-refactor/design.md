## Context

### 现状盘点 (commit e7833f1 前)

```
Channel                              Type            用途                  现状
─────────────────────────────────────────────────────────────────────────────────
state.event_tx (broadcast 1024)      broadcast       ServerEvent → WS       ✅ 已经是事件总线雏形
state.frame_tx (broadcast 100)        broadcast       FrameInfo → WS/web     ✅ 已经是
state.llm_text_tx (mpsc Unbo)         tokio mpsc      LLM 输入                ❌ 应该合并
voice._rx (mpsc 占位字段)             std mpsc        ASR 文本                ❌ 死代码, 已 commit 修
voice.asr_text_rx (Arc<Mutex<...>>)   std mpsc        ASR 文本外抛            ❌ 应该合并
voice.text_tx→text_rx (mpsc)         std mpsc        ASR 文本 → 桥接         ❌ 应该合并
voice.audio_tx→audio_rx (sync 4)    std sync        cpal 音频流             ✅ 保留
state.bot_tx (sync 1)                std sync        关节指令 → USB         ✅ 保留
ws.out_tx/out_rx (unbo)              tokio mpsc      WS 单连接输出           ✅ 保留
ws.tx/rx (sync 1)                    std sync        关节配置回送            ✅ 保留
```

### 架构目标

```
                   ┌──────────────────────────────────────┐
                   │           tokio::sync::broadcast     │
                   │             <BusEvent>(1024)        │
                   │                                      │
   publishers      │    subscribers                       │
   ─────────────   │    ──────────                        │
   ws.rs   ──┐     │   ws.rs (ServerEvent + Volume +...) │
   state.rs ──┤    │   spawn_llm_thread (AsrText)        │
   asr.rs   ─┼────▶│   spawn_tts_trigger (LlmReply)     │
   face ─────┤     │   spawn_face_tracking (FaceDetect) │
   voice ───┘     │   preview (Volume / Face)            │
                   │                                      │
                   └──────────────────────────────────────┘
   data flows (NOT through bus)
   ─────────────────────────────────────
   cpal audio:   sync_channel<Vec<f32>>(4)
   camera frame: broadcast<FrameInfo>(100)
   ws duplex:    per-connection mpsc::unbounded_channel
   joint cmd:    sync_channel(1)
```

## Goals / Non-Goals

**Goals:**
- `BusEvent` 单一枚举 + `EventBus` 封装, 替代事件流相关的多类 channel
- LLM / TTS / WS / face tracking 等都从 bus 订阅, 不再各自维护 sender
- 测试覆盖: `EventBus` 单测 + ASR → LLM → TTS 集成测试
- 新增订阅者成本降到 O(1 行 `bus.subscribe()`)
- 协议层 `ServerEvent` 不变 (WS 边界仍序列化 ServerEvent)

**Non-Goals:**
- 不重构数据流 (audio / video / WS 双向 / joint cmd)
- 不引入 actor 框架 / crossbeam
- 不改 `LlmManager` / `LlmTrait` 内部
- 不改 `ServerEvent` 协议
- 不动 `crates/ele_bot_client`, `crates/ele_bot_proto`

## Decisions

### D1: BusEvent 大枚举 + 单 bus

```rust,ignore
// crates/ele_bot_server/src/event_bus.rs

use serde::{Deserialize, Serialize};
use crate::proto::{ServerEvent, FrameInfo};

#[derive(Debug, Clone)]
pub enum BusEvent {
    /// 协议层服务端事件 (TUI 显示)
    ServerEvent(ServerEvent),
    /// ASR 识别文本 (内部 LLM 消费)
    AsrText(String),
    /// LLM 对话回复 (内部 TTS 消费)
    LlmReply(String),
    /// LLM 处理中标志
    LlmProcessing { is_processing: bool },
    /// 音量 (内部 UI 消费, 替代 ws.rs 50ms tick 轮询)
    Volume(i32),
}

#[derive(Clone)]
pub struct EventBus {
    inner: tokio::sync::broadcast::Sender<BusEvent>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(capacity);
        Self { inner: tx }
    }
    
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<BusEvent> {
        self.inner.subscribe()
    }
    
    pub fn publish(&self, event: BusEvent) {
        let _ = self.inner.send(event);
    }
    
    pub fn subscriber_count(&self) -> usize {
        self.inner.receiver_count()
    }
}
```

**理由**: 单 bus + match 过滤, 不需要路由表; 枚举就是路由. broadcast 原生多订阅者.

### D2: bus 容量 = 1024

跟现有 `event_tx` 一致. LLM 慢 + UI 渲染慢 + GC pause 抖动都不会 Lagged. 1024 个事件约等于 ~30s@30Hz, 远超事件流峰值 30/s.

### D3: LLM thread 迁 tokio task

```rust,ignore
// 旧: std::thread::spawn + mpsc::blocking_recv
// 新: tokio::spawn + broadcast::Receiver::recv().await

fn spawn_llm_thread(self: &Arc<Self>) {
    let state = self.clone();
    let mut rx = state.bus_tx.subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(BusEvent::AsrText(text)) => {
                    state.process_llm_text(&text).await;
                }
                Ok(_) => continue,  // 其它 variant 不关心
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    log::warn!("LLM thread lagged, dropped {n} events");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}
```

**理由**: `broadcast::Receiver::recv()` 是 async. 同步线程要用得 try_recv + sleep polling, 破坏一致性. LLM thread 迁 tokio 是干净选择. `process_llm_text` 内部仍然 `state.llm.lock()` (同步 Mutex), 在 tokio task 里短暂阻塞 OK (不持有 await 点).

### D4: VoiceManager 删除 ASR 文本外抛 channel

```rust,ignore
// 旧:
//   voice.asr_text_rx + take_asr_text_rx() + spawn_asr_bridge_thread
//   桥接: voice._rx → llm_text_tx
//
// 新:
//   recognition_thread 直接 bus.publish(AsrText(text))
//   不再需要 _rx / asr_text_rx / bridge
```

**理由**: ASR 文本是事件流, 直接走 bus. VoiceManager 不持有 bus (避免循环引用). recognition_thread 接收 `EventBus` 引用作为参数.

### D5: TTS 触发改 TTS trigger thread

```rust,ignore
fn spawn_tts_trigger_thread(self: &Arc<Self>) {
    let state = self.clone();
    let mut rx = state.bus_tx.subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(BusEvent::LlmReply(text)) => {
                    if text.is_empty() { continue; }
                    if let Some(voice) = state.voice.lock().unwrap().clone() {
                        tokio::task::spawn_blocking(move || {
                            if let Err(e) = voice.speak(&text, 1.0, None) {
                                log::warn!("TTS playback failed: {e:?}");
                            }
                        });
                    }
                }
                Ok(_) => continue,
                Err(broadcast::error::RecvError::Lagged(n)) => log::warn!("TTS trigger lagged {n}"),
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}
```

**理由**: TTS 触发逻辑从 `spawn_llm_thread` 抽出来, 各管各的. LLM 只关心 chat + analyze_mood + 发布事件.

### D6: ws.rs 订阅过滤

```rust,ignore
let mut bus_rx = state.bus_tx.subscribe();
let sub_task = tokio::spawn(async move {
    loop {
        match bus_rx.recv().await {
            Ok(BusEvent::ServerEvent(se)) => {
                if out_tx_clone.send(se).is_err() { break; }
            }
            Ok(BusEvent::Volume(v)) => {
                if out_tx_clone.send(ServerEvent::Volume { value: v }).is_err() { break; }
            }
            Ok(BusEvent::LlmReply(_)) | Ok(BusEvent::AsrText(_)) => continue,  // 内部用, 不外发
            Ok(_) => continue,
            Err(broadcast::error::RecvError::Lagged(n)) => log::debug!("ws lagged {n}"),
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
});
```

**理由**: WS 只关心能给用户看的事件. BusEvent::Volume 替代 ws.rs:140 50ms tick 轮询 (新加分发音量事件, ws 删 tick).

### D7: ServerEvent 协议保持不变

WS 序列化仍用 `ServerEvent::to_json()`. bus 内部枚举 `BusEvent::ServerEvent(ServerEvent)` 是包装, 不是替代. 旧客户端零影响.

### D8: 测试覆盖

新增单测:
1. `EventBus` publish + subscribe 单线程
2. `EventBus` 多订阅者隔离 (subscriber A 只看到自己 subscribe 后的事件)
3. `EventBus` Lagged 处理
4. (集成) 模拟 ASR text → bus → LLM stub → bus → TTS stub, 验证链路通

## Risks / Trade-offs

- [R1: `voice` 字段还是 `Mutex<Option<Arc<VoiceManager>>>`, TTS trigger thread 在 voice 为 None 时静默跳过. 跟现状一致.] → Mitigation: log warn, 跟现状策略相同.
- [R2: LLM thread 迁 tokio 后, LLM 内部的 candle / async_openai 阻塞调用是否兼容?] → Mitigation: candle 是同步的, async_openai 是异步的 — OnlineLlm::chat 已用 `tokio::runtime::Runtime::new().block_on` 包装. 我们这里再套一层 `tokio::spawn + block_on` 会双重 runtime. **解决: 把 OnlineLlm::chat 改成真正的 async (tokio + block_on 不需要)**, 或 LLM thread 保持 std::thread + 用 `try_recv()` polling (这破坏 D3). **需要 spike**: 跑现有 OnlineLlm 在 tokio runtime 里看是否 panic.
- [R3: `ServerEvent` 内部字段比如 `LlmResponse { reply_text }` 是用户协议层; 而 bus 内部 `BusEvent::LlmReply(String)` 是简化版. ws.rs 需要把 LlmReply 转回 LlmResponse 才能发给 TUI.] → Mitigation: 转换在 ws.rs filter 里一行代码.
- [R4: ws.rs 现在每 50ms tick 推音量. 重构后音量改 bus.publish → ws.rs 订阅, ws.rs 50ms tick 可以删. 但 tick 还做 LCD 帧推送 (state.generate_lcd_frame), 不能整个删.] → Mitigation: tick 只剩 LCD 帧, 音量从 bus 来. 这部分是收益 (更准的音量推送时机).
- [R5: 重构跨度大, 容易引入新 bug.] → Mitigation: 每步跑三件套 + 现有 ignored 测试 (`test_recognition_with_audio_file` 等) 不能 regress. 最后跑一次 production smoke test (server + TUI + 说话听回复).

## Migration Plan

无. 重构后协议不变, 旧客户端不影响. 服务端热重启.

## Open Questions

- [O1] `BusEvent::Volume` 是替代 ws.rs 50ms tick 推送, 还是同时存在? — 设计为替代 (R4). 音量更新改在 `recognition_loop` 末尾 / 单独的音量更新点 publish.
- [O2] `BusEvent::LlmReply` 是否要带 `mood: Mood` 字段, 让 TTS trigger 时顺便设表情? — 初版只带 String, 表情由 LLM 单独发 `ServerEvent::LlmResponse { mood, actions }`. 简单清晰.
- [O3] `BusEvent::JointState` 是否要新增, 替代 ws.rs 的 `broadcast_joint_state()`? — 初版不加, ws.rs 50ms tick 保留推 LCD frame. 后续 PR 再说.