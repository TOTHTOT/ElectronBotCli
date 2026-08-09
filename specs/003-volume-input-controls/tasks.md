# Tasks: 设置页音量调节与测试输入框优化

**Input**: plan.md / spec.md / research.md / data-model.md / contracts/ @ specs/003-volume-input-controls/

**Tests**: 宪法要求关键模块带测试，测试随实现任务同任务内完成（proto 兼容性、
增益乘法、TextInput 编辑核心均为纯逻辑，必须单测）。

## Phase 1: Foundational (Blocking Prerequisites)

- [X] T001 `crates/ele_bot_proto/src/types.rs`: `AppConfig` 新增
  `speaker_volume: u8` / `mic_volume: u8`，均 `#[serde(default = "default_100")]`
  （始终序列化，不带 skip）；补单测：旧 TOML 无新字段解析 = default(100/100)、
  新字段 JSON roundtrip。契约见 contracts/protocol.md §1

**Checkpoint**: proto 编译通过，US1/US2 可并行开工

---

## Phase 2: User Story 1 - 设置页音量条 (Priority: P1) 🎯 MVP

**Goal**: 设置页扬声器/麦克风两个音量条，←→ 调节实时生效于机器人端并持久化

**Independent Test**: quickstart.md 场景 2/3（实机调音量可感知变化，重启保持）

- [X] T002 [US1] `crates/ele_bot_server/src/media/voice/mod.rs`: `VoiceManager`
  新增 `speaker_gain` / `mic_gain: Arc<AtomicU32>`（f32 bits，初值从 AppConfig
  读）+ `set_speaker_gain(&self, f32)` / `set_mic_gain(&self, f32)`（rustdoc 齐全）
- [X] T003 [P] [US1] `crates/ele_bot_server/src/media/voice/tts.rs`: `TtsPlayer`
  持 speaker_gain 原子量（`new` 加参数），`play()` / `start_streaming()` 创建
  `Player` 后 `player.set_volume(gain)`（research.md 决策 1）
- [X] T004 [P] [US1] `crates/ele_bot_server/src/media/voice/asr.rs`:
  `build_asr_stream` / `process_audio_chunk` 加 gain 参数，转 f32 后逐样本
  乘增益并 clamp [-1.0, 1.0]，再做峰值/downmix（电平 = 增益后信号）；
  单测：增益 0.5 峰值减半、增益 2.0 clamp 不超限（research.md 决策 2）
- [X] T005 [US1] `crates/ele_bot_server/src/state.rs`: `set_config` 检测
  speaker_volume/mic_volume 变化 → 调 VoiceManager setter 热更新（**不触发**
  rebuild_voice），clamp [0,100]；音量链路走现有 cfg.save() + 广播 Config
- [X] T006 [US1] `crates/ele_bot_client/src/ui/viewmodel/settings.rs`: 设置项
  新增两行（扬声器音量/麦克风音量，插在输出设备与 TTS 项之间），显示值取自
  config mirror；viewmodel 单测：行存在性与百分比格式
- [X] T007 [US1] `crates/ele_bot_client/src/ui/pages/settings.rs`: 音量条渲染
  （复用 device_status.rs render_bar 风格）：扬声器行 = 增益条+百分比；
  麦克风行 = 增益条+百分比+实时电平指示（`server.volume`）；
  操作说明栏选中音量行时切换为 `←→ 调节音量`（contracts/settings-ui.md §1）
- [X] T008 [US1] `crates/ele_bot_client/src/input/settings.rs`: 音量行选中时
  `←`/`→` ±5%（clamp [0,100]）改本地 config mirror 并回写 `SetConfig`；
  音量行 `Enter` 无操作（不进 EditField overlay）

**Checkpoint**: US1 完整可验（quickstart 场景 2/3），US2 不受影响

---

## Phase 3: User Story 2 - TTS/LLM 测试输入框完整编辑 (Priority: P2)

**Goal**: 两测试页输入框支持退格/Delete/光标移动/中间插删/一键清空

**Independent Test**: quickstart.md 场景 4（输入-移光标-插删-清空-提交）

- [X] T009 [US2] `crates/ele_bot_client/src/ui_components/text_input.rs`（新建）:
  `TextInput { buffer: String, cursor: usize(char 索引) }`，方法
  `insert_char/delete_back/delete_forward/move_left/move_right/move_to_start/
  move_to_end/clear/before_cursor/after_cursor`，不变量 cursor ∈ [0, char数]；
  `ui_components/mod.rs` 导出；单测：中文插删、空 buffer 边界、Home/End、
  clear、before/after 切分（research.md 决策 4）
- [X] T010 [US2] `crates/ele_bot_client/src/ui/pages/tts_test.rs` +
  `crates/ele_bot_client/src/ui/pages/llm_test.rs`: state 的
  `input_text: String` 迁移为 `input: TextInput`；提交路径取 buffer 全文
- [X] T011 [P] [US2] `crates/ele_bot_client/src/input/tts_test.rs`: 按键映射到
  TextInput（Char 插入/Backspace/Delete/←→/Home/End/Ctrl+U 清空）；
  `↑`/`↓`/`+`/`-` 调速、`M` 切流式保留不冲突（contracts/settings-ui.md §2）
- [X] T012 [P] [US2] `crates/ele_bot_client/src/input/llm_test.rs`: 同上映射；
  `F2` 清空记忆保留
- [X] T013 [P] [US2] `crates/ele_bot_client/src/ui/pages/tts_test.rs`: 输入框
  块字符 caret 渲染（before/caret/after 三段，策略同 settings.rs:96 注释），
  超宽横向滚动保持 caret 可见
- [X] T014 [P] [US2] `crates/ele_bot_client/src/ui/pages/llm_test.rs`: 同上渲染

**Checkpoint**: US1 + US2 均独立可用

---

## Phase 4: Polish & Cross-Cutting

- [X] T015 质量门禁: `cargo fmt --all && cargo clippy --all-features --all-targets -- -D warnings && cargo test -p ele_bot_proto -p ele_bot_server -p ele_bot_client`
- [X] T016 [US1] 实机验证（lckfb 部署 + quickstart 场景 2/3），用户听感/电平确认
- [X] T017 quickstart 场景 4/5 人工验证；勾选 spec checklist 适用项

---

## Dependencies & Execution Order

- **T001** 阻塞 US1（音量字段）；US2（T009-T014）不依赖 T001，可与 US1 并行
- US1 内：T002 → T003/T004（可并行）→ T005；T006 → T007/T008（T008 依赖
  T006 的 viewmodel 行定义；T007/T008 同文件不同函数，顺序执行）
- US2 内：T009 → T010 → T011/T012/T013/T014（四个文件可并行）
- T015 → T016 → T017 顺序收尾

## Parallel Example

```bash
# Foundational 后, server 侧两文件并行:
Task: "T003 TtsPlayer set_volume (tts.rs)"
Task: "T004 asr 采集增益 (asr.rs)"

# US2 state 迁移后, 四个文件并行:
Task: "T011 input/tts_test.rs 按键映射"
Task: "T012 input/llm_test.rs 按键映射"
Task: "T013 tts_test.rs 页渲染"
Task: "T014 llm_test.rs 页渲染"
```

## Implementation Strategy

- **MVP**: T001 + US1（T002-T008）即可独立交付音量调节
- US2 与 US1 无耦合，可整体后置或并行
