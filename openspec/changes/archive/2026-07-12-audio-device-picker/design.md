## Context

设置页 `crates/ele_bot_client/src/ui/pages/settings.rs` 目前只有 3 行配置项 (Wifi 名称 / Wifi 密码 / 麦克风名称), 都走 `EditField` 文本编辑. 麦克风行让用户手敲设备名, 服务端 `voice::find_input_device` 做 exact match (`crates/ele_bot_server/src/media/voice/mod.rs:259`). 实际用户体验差:
1. 设备名往往很长且包含不易记忆的字符 (例 `麦克风阵列 (Realtek High Definition Audio)`)
2. 同名设备无法区分 (cpal 上偶有发生)
3. 扬声器选择 (`output_device` 字段已存在于 `AppConfig`) 完全没暴露在 UI

服务端 `list_input_devices()` / `list_output_devices()` 已实现 (`voice/mod.rs:307,328`) 并返回 `DeviceInfo { name, display }`, `display` 格式为 `"name (driver, Nch, NHz)"`, 但 TUI 从未调用.

约束:
- 输入层派发契约 (CLAUDE.md): 任何按键翻译必须走 `AppEvent` → `handle_event`, 不能绕过.
- 当前 `Route::Settings { selected, editing }` 已有子状态字段, 新增 picker 子模式沿用此风格.
- `VoiceManager` 启动时一次性构造 (`state.rs:97`), 重建路径无现成 API, 需要新增.

## Goals / Non-Goals

**Goals:**
- 设置页麦克风/扬声器改成设备列表选择, 显示 name + driver + channels + sample_rate
- 列表第一项为"系统默认" (空 name)
- 选择后**立即重建 `VoiceManager`**, 无需重启
- 设置页启动时拉一次设备列表; 提供 `[R]` 手动刷新
- 设备列表为空时 UI 显示 `<无可用设备>`, 仍允许保持当前选择

**Non-Goals:**
- 不存 driver 到 `AppConfig` (维持只存 name 的最小改动)
- 不改 ASR / TTS 模型加载路径; 只暴露重建入口
- 不引入 fuzzy / 子串匹配; 选择即选定, 匹配逻辑沿用现有 exact match
- 不改 `EditField` overlay 行为 (它继续处理 Wifi 等文本输入项)

## Decisions

### D1: UI 用 Settings 子模式 (A 方案)

`Route::Settings` 增加 `selecting: Option<DeviceKind>` 字段. 进入 picker 时设置 `selecting = Some(Mic | Speaker)`, 离开时清空. 渲染层根据 `selecting` 切换两种布局: 列表 vs 子选择器.

**为什么不用新 Route 变体**: Settings 已有 `selected/editing` 子状态, picker 与它们是同一生命周期. 把 picker 作为同变体的字段而不是新变体, 让 Esc 行为统一 (清 selecting 而不需要 route 转换). 与现有 `DeviceControl::Idle/Active` 子模式风格一致 (`route.rs:9`).

**为什么不用 Overlay**: Overlay 是"浮在当前页之上"的概念, picker 是"换一种渲染模式"的概念. 复用 overlay 会让 `handle_overlay` 加一堆分支, 而 `handle_settings` 已经存在; 走子模式更局部.

### D2: 服务端立即热重建 VoiceManager

新增 `SharedState::rebuild_voice(&self) -> anyhow::Result<()>`. 步骤:
1. 读 `self.config.read()` 取当前 `AppConfig`
2. 调 `Self::init_voice(&config)` 构造新的 `VoiceManager` (私有方法改为 `pub(crate)` 或内联实现)
3. `let mut g = self.voice.lock().unwrap(); *g = Some(Arc::new(new));` — 旧 `Arc<VoiceManager>` 引用计数归零时自动 Drop
4. 旧 Drop 链路: `cpal::Stream` Drop 停流 → `audio_tx` 所有克隆 Drop → `recognition_thread` 的 `audio_rx.recv()` 返回 Err → thread 退出 → `text_tx` Drop. 不需要手动 join (thread 是 `std::thread::spawn`, detached).
5. 失败时回退: 若 `init_voice` 返回 Err, 不替换, 广播 `ServerEvent::Error`.

**为什么不持久化优先**: 用户期望"选了立刻生效", 不持久化优先等于假动作. VoiceManager::new 是同步调用, 不阻塞 ws 循环太久 (模型已加载, 只是新建 stream / tts player). 一次重建 < 1s.

### D3: 协议加 4 条消息, 不复用 SetConfig

新增:
- `ClientMessage::ListInputDevices` / `ListOutputDevices`
- `ServerEvent::InputDevices { devices: Vec<DeviceInfoDto> }` / `OutputDevices { devices: Vec<DeviceInfoDto> }`
- `pub struct DeviceInfoDto { name: String, display: String, driver: Option<String> }`

**为什么不用 SetConfig 附带 devices 字段**: devices 是运行时查询数据, 不是配置. SetConfig 应只承载可持久化字段. 混在一起会让 AppConfig 变成"既要存又要查"的双重角色, 序列化也变重.

设备选择本身仍走 `SetConfig { config: AppConfig { speech_name, output_device, .. } }` — 只 name 写入 config. 重建触发由服务端在 `set_config` 里检测"input/output device 字段是否变化"决定是否调 `rebuild_voice`.

### D4: 客户端缓存与刷新时机

`App` 增加 `input_devices: Vec<DeviceInfoDto>` / `output_devices: Vec<DeviceInfoDto>` (Vec 持有, 不放进 Route).

时机:
- 进入 `Route::Settings` (从 Nav 进入, 或 App 启动时) → 发 `ListInputDevices` + `ListOutputDevices`
- 设置页内按 `[R]` → 重新发两条
- 收到 `InputDevices` / `OutputDevices` → 写入 `App` 缓存, 触发 UI 重绘

"系统默认"项在客户端本地构造 (index 0, `name=""`, `display="<系统默认>"`), 不走服务端. 这样服务端枚举逻辑保持纯净 (`list_*_devices` 只返回真实设备).

### D5: AppConfig 不动

`AppConfig.speech_name: String` / `output_device: String` 已存在, 选设备时只更新这两个字段, 整 config 走 `SetConfig`. 不增加 `speech_driver` 等新字段. 同名设备的歧义通过 UI 上 display 的 driver 段化解.

## Risks / Trade-offs

**[R1] TTS 播放中切换输出设备 → 正在播的音频被截断**
→ Mitigation: UI 操作流程上不阻塞; 若用户主动切, 视为可接受的瞬断. 不在切换前主动 finish 旧 stream (会引入额外状态机).

**[R2] 重建 VoiceManager 时若 cpal 拒绝打开新设备, 当前输入/输出静默失效**
→ Mitigation: `rebuild_voice` 失败时不替换旧的 `Mutex<Option<Arc<VoiceManager>>>`, 广播 `ServerEvent::Error` 给客户端; UI 在 picker 顶显示错误并保留旧选择.

**[R3] cpal 在某些平台上枚举慢 (几百 ms)**
→ Mitigation: 设备列表拉取走 ws 异步请求, 不阻塞 UI 帧率; 拉取期间显示 `加载中...`.

**[R4] 设置页启动时与 Config 推送时序竞态**
→ 客户端进入 Settings 时发 List*, 同时可能收到初始 `ServerEvent::Config`. 两者独立, 不冲突. 但若用户在收到设备列表前就按 Enter 进入 picker, 当前 index 越界.
→ Mitigation: picker 进入时若列表为空, 显示 `<加载中或无可用设备>` 并把 Enter 屏蔽掉, 只允许 Esc 退出.

**[R5] `init_voice` 失败 (模型未加载) 时 `voice` 一直是 `None`**
→ 既有行为, 不在本 change 修复. 但 picker 进入时应检查 `voice` 是否曾初始化过 (用于判断能否重建). 不必暴露这个细节给 UI, 让 `rebuild_voice` 自行 bail.

## Migration Plan

无. 这是纯增量功能, 不修改 `AppConfig` 字段, 不动持久化格式 (`config.toml` 兼容), 不破坏现有 CLI 行为.

回滚: 单 commit revert 即可.

## Open Questions

无. 用户已确认:
- 进入 Settings 时拉一次 (问题 1) ✅
- 空列表兜底显示 (问题 2) ✅
- `[R]` 刷新 (问题 3) ✅
- "系统默认" 作为列表首项 (问题 4) ✅
- UI 形态 A (子路由) ✅
- 立即热重建 (Q2) ✅
- 不存 driver (Q3) ✅