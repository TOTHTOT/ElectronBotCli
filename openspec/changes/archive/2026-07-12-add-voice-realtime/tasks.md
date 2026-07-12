# Tasks: add-voice-realtime

## 1. proto 层 — Volume 协议

- [x] 1.1 在 `crates/ele_bot_proto/src/messages.rs` 的 `ServerEvent` 枚举新增 `Volume { value: i32 }` 变体, 带 `///` rustdoc 说明 `value` 是 0..=100 归一化峰值音量. 完成后跑三件套
- [x] 1.2 在 `messages.rs::tests` 增补 roundtrip 测试: `ServerEvent::Volume { value: 42 }` 序列化 → 反序列化后字段一致. 完成后跑三件套

## 2. server 层 — VoiceManager 取消信号

- [x] 2.1 在 `crates/ele_bot_server/src/media/voice/mod.rs` 的 `VoiceManager` 结构体新增字段 `running: Arc<AtomicBool>` (初始 true). 完成后跑三件套
- [x] 2.2 在 `VoiceManager` 上新增 `pub fn running(&self) -> Arc<AtomicBool>` 与 `pub fn is_running(&self) -> bool`, 各带 `///` rustdoc. 完成后跑三件套
- [x] 2.3 在 `VoiceManager::new` 里把 `self.running.clone()` 传给后续 `thread::spawn` 闭包 (供 asr.rs 用). 完成后跑三件套

## 3. server 层 — ASR 线程 recv_timeout

- [x] 3.1 在 `crates/ele_bot_server/src/media/voice/asr.rs` 把 `recognition_loop` 的 `for samples in audio_rx` 改为 `loop { match audio_rx.recv_timeout(Duration::from_millis(50)) {...} }`; 在 `Timeout` 分支检查 `running` 标志, false 时返回; 在 `Disconnected` 分支也返回. 完成后跑三件套
- [x] 3.2 把 `recognition_thread` 入口签名加 `running: Arc<AtomicBool>` 参数, 转发给 `recognition_loop`. 完成后跑三件套
- [x] 3.3 在 `crates/ele_bot_server/src/media/voice/mod.rs::VoiceManager::new` 的 `thread::spawn` 闭包里把 `self.running.clone()` 传给 `recognition_thread`. 完成后跑三件套

## 4. server 层 — 周期广播 + 软重建

- [x] 4.1 在 `crates/ele_bot_server/src/ws.rs` 的 `frame_interval.tick()` 分支末尾增加音量广播: 读 `state.voice` 当前 volume (`voice.volume().load(Relaxed)`, voice 为 None 时 0), 调 `state.event_tx.send(ServerEvent::Volume { value })`. 补行内注释说明与 LCD 帧共用 tick. 完成后跑三件套
- [x] 4.2 在 `crates/ele_bot_server/src/state.rs::rebuild_voice` 改造: 先 `voice.lock().take()` 拿旧实例, 旧实例 `running.store(false, Relaxed)`, `std::thread::sleep(Duration::from_millis(60))` 给退出窗口, 然后再 `init_voice` + `replace`. 补 `///` rustdoc 说明软替换语义. 完成后跑三件套

## 5. client 层 — 接收 Volume

- [x] 5.1 在 `crates/ele_bot_client/src/app/mod.rs::apply_event` 增加 `ServerEvent::Volume { value }` 分支, 写 `server.volume = value`. 完成后跑三件套
- [x] 5.2 跑 `cargo fmt --all && cargo clippy --all-features --all-targets -- -D warnings && cargo check --all-features --all-targets && cargo test -p ele_bot_proto`, 全过
- [x] 5.3 手动验证脚本: 启动 server + client, 进设备状态页, 对着麦克风说话音量条实时刷新; 切到设置页换麦克风, 旧 ASR 线程 50ms 内退出 (服务端 log 出现 "ASR 线程收到取消信号, 退出"). 记录到 PR 描述