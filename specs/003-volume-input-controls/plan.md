# Implementation Plan: 设置页音量调节与测试输入框优化

**Branch**: `003-volume-input-controls` | **Date**: 2026-08-09 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `/specs/003-volume-input-controls/spec.md`

## Summary

设置页新增扬声器/麦克风两个音量条（键盘 ←→ 调节，实时生效并持久化），
TTS/LLM 测试页输入框从"只能追加 + 整字退格"升级为支持光标移动/插删/清空的
完整编辑控件。

技术路线（详见 [research.md](research.md)）：音量全部走**软件增益**，跨平台零
设备特判 — 扬声器用 rodio `Player::set_volume`，麦克风在 ASR 采集回调里乘增益；
持久化复用现有 `AppConfig`/`SetConfig`/`config.toml` 链路（新增两个带 serde
默认值的字段，协议只增不改）；麦克风实时电平**零协议改动**（`ServerEvent::Volume`
已在广播，client 已接收）；输入框在 `ui_components` 新建 `TextInput` 编辑核心，
复用设置页已验证的 char 级编辑 + 块字符 caret 渲染策略。

## Technical Context

**Language/Version**: Rust 1.94（workspace 现状）

**Primary Dependencies**:
- server: rodio 0.22.2（`Player::set_volume`，已在依赖树）、cpal 0.18（采集）
- client: ratatui + crossterm（现状）、`unicode-width`（已用于 settings.rs）
- **无新增第三方 crate**

**Storage**: `config.toml`（`AppConfig::save()`，proto/types.rs，现状机制）

**Testing**: `cargo test -p ele_bot_proto / -p ele_bot_server / -p ele_bot_client`

**Target Platform**: server = RK3566 (aarch64-linux-gnu) + macOS/Windows/Linux 桌面；
client = 跨平台终端 TUI

**Project Type**: 嵌入式机器人 server + 终端 TUI client（WebSocket 协议）

**Performance Goals**: 音量调节按键 → 生效 < 200ms（本地原子量更新，无重建）；
实时电平沿用现有节流（asr.rs `VOLUME_PUBLISH_MIN_INTERVAL_MS`）

**Constraints**:
- 跨平台：禁止 ALSA/amixer 等 Linux-only 路径，全部软件增益
- 宪法 IV：proto 只增不改，新旧 client/server 交叉不报错
- 宪法 I：client 输入必须走 `input/mod.rs` 派发层

**Scale/Scope**: server 3 文件 + proto 1 文件 + client ~6 文件

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| 原则 | 判定 | 说明 |
|------|------|------|
| I. 输入派发契约 | PASS | 测试页按键仍走 `input/tts_test.rs` / `input/llm_test.rs` / `input/settings.rs` 派发，不新建旁路 |
| II. rustdoc 强制 | PASS | `TextInput`、音量相关 pub 项全部补 rustdoc |
| III. 质量门禁 | PASS | 提交前 fmt + clippy -D warnings + check（+ 相关 crate test） |
| IV. 协议双向兼容 | PASS（附已知限制） | `AppConfig` 新增字段带 `#[serde(default)]`；旧 client 回写 SetConfig 会把音量重置为默认 100 — 无法区分"旧 client"与"显式设 100"，作为已知限制记录在 data-model.md，不违反"交叉不报错" |
| V. 最小改动 | PASS | 零新增依赖；增益用 `Arc<AtomicU32>` 热更新，不重建音频流；不复用改造 route.rs `EditField`（避免动已工作的设置 overlay，见 research.md 决策 4） |

Phase 1 设计后复核：无新增违规。

## Project Structure

### Documentation (this feature)

```text
specs/003-volume-input-controls/
├── plan.md              # 本文件
├── research.md          # Phase 0 输出
├── data-model.md        # Phase 1 输出
├── quickstart.md        # Phase 1 输出
├── contracts/           # Phase 1 输出
│   ├── protocol.md      # AppConfig 新字段 + SetConfig/Volume 事件契约
│   └── settings-ui.md   # 设置页音量条 + 测试输入框键盘契约
└── tasks.md             # /skill:speckit-tasks 输出
```

### Source Code (repository root)

```text
crates/ele_bot_proto/src/
└── types.rs             # AppConfig += speaker_volume / mic_volume (serde default 100)

crates/ele_bot_server/src/
├── state.rs             # set_config: 音量变化走原子量热更新, 不触发 rebuild
└── media/voice/
    ├── mod.rs           # VoiceManager 持 speaker/mic 增益原子量 + setter
    ├── tts.rs           # play()/start_streaming() 创建 Player 后 set_volume(gain)
    └── asr.rs           # process_audio_chunk 乘采集增益 (clamp), build_asr_stream 加 gain 参数

crates/ele_bot_client/src/
├── ui_components/
│   ├── mod.rs           # pub mod text_input
│   └── text_input.rs    # TextInput 编辑核心 (char 级 cursor, 新建)
├── ui/pages/
│   ├── settings.rs      # 音量条行渲染 (增益条 + 麦克风电平叠加)
│   ├── tts_test.rs      # 输入框块字符 caret 渲染 + 横向滚动
│   └── llm_test.rs      # 同上
├── ui/viewmodel/
│   └── settings.rs      # 设置项新增两个 Volume 行 (只读显示值由 config mirror 提供)
├── input/
│   ├── settings.rs      # 音量行 ←→ 调节 → 发 SetConfig
│   ├── tts_test.rs      # 按键 → TextInput 方法 (Backspace/Delete/←→/Home/End/Ctrl+U)
│   └── llm_test.rs      # 同上
└── app/mod.rs           # 发送 SetConfig 复用现有路径; server.volume 已有
```

**Structure Decision**: 沿用现有三 crate 分层。音量状态的唯一权威源是服务端
`AppConfig`（config.toml）；client 只作为控制入口 + 显示镜像，不另存副本。
增益热更新用 `Arc<AtomicU32>`（存 f32 bits）避免音频流重建。

## Complexity Tracking

无违规需论证。
