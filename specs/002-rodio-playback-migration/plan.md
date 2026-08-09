# Implementation Plan: rodio 替换手写 cpal 播放层

**Branch**: `002-rodio-playback-migration` | **Date**: 2026-08-09 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `/specs/002-rodio-playback-migration/spec.md`

## Summary

把 `crates/ele_bot_server/src/media/voice/tts.rs` 的 TTS 播放层从手写 cpal
回调 (格式探测/双回调变体/兜底重试, 约 200 行) 迁移到 rodio 0.22
(`playback`-only feature)。rodio 的 mixer 对任意输入源自动做位宽/声道/
采样率转换, `DeviceSinkBuilder::open_sink_or_fallback` 内置设备配置回退
枚举, 覆盖 PCM2912A 这类只收 S16_LE 的 USB 声卡, 同时免疫未来更换不同
采样率 TTS 模型的问题。对外契约 (`TtsPlayer` / `StreamPlayerHandle` API
与播放完成语义) 保持不变, `VoiceManager` 无改动。

## Technical Context

**Language/Version**: Rust 1.94 (workspace), rodio 0.22.2 (rust-version 1.87, 满足)

**Primary Dependencies**: rodio 0.22.2 (`default-features = false, features = ["playback"]`),
cpal 0.18 (rodio 复用, 设备枚举/选择仍走现有 `find_output_device`)

**Storage**: N/A

**Testing**: cargo test (现有 35 项 + 播放层单元测试), 实机验证 (macOS + lckfb/Rockchip)

**Target Platform**: macOS (开发), Windows / Linux x86 (可构建), Linux ARM64
嵌入式 (Rockchip RK3566/CM3, 主部署目标)

**Project Type**: embedded daemon (长期驻留服务, WebSocket 协议不变)

**Performance Goals**: 不回退 — 播放无提前截断, 流式首包延迟不劣于现状

**Constraints**: 只引入播放核心, 不拉解码器; 设备独占语义变化 (流常驻 vs
每次播放开关) 需文档化; `OwnedOutputStream` 删除前须确认无其他使用方

**Scale/Scope**: 单文件重写 (`tts.rs` 播放层, ~200 行 → ~60 行),
`VoiceManager` 零改动, proto 零改动

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| 原则 | 判定 | 说明 |
|------|------|------|
| I. 输入派发契约 | N/A | 不涉及 client 输入 |
| II. rustdoc 强制 | PASS | 新播放层公开项继续带中文 rustdoc + 边界说明 |
| III. 质量门禁 | PASS | 提交前 fmt/clippy/-D warnings/check 三件套 |
| IV. 协议双向兼容 | PASS | 纯 server 内部实现, `ele_bot_proto` 零改动 |
| V. 最小改动/依赖 | JUSTIFIED | 新增 rodio 依赖: 已按例外条款上报用户并获批准; feature 裁剪到 playback-only, 不引入解码器 |

**Gate 结论**: PASS (V 的依赖新增已经用户确认, 见 spec Assumptions)

## Project Structure

### Documentation (this feature)

```text
specs/002-rodio-playback-migration/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
└── contracts/           # N/A — 纯内部重构, 无外部接口变更 (proto 不变)
```

### Source Code (repository root)

```text
crates/ele_bot_server/
├── Cargo.toml                       # +rodio (playback-only)
└── src/media/voice/
    ├── mod.rs                       # VoiceManager (零改动, 仅删除不再用的引用)
    └── tts.rs                       # 播放层重写: TtsPlayer/StreamPlayerHandle
                                     #   内部换 rodio, 公开 API 不变
```

**Structure Decision**: 单文件重写 `tts.rs`, 不动模块划分。`find_output_device`
(设备枚举/按 id 选择) 保留在 `voice/mod.rs` 继续复用 — rodio 的
`DeviceSinkBuilder::from_device` 接受 cpal `Device`, 与现有选择逻辑无缝衔接。

## 设计要点 (Phase 1 结论, 详见 research.md)

| 现状 (cpal 手写) | 迁移后 (rodio) |
|---|---|
| `TtsPlayer { device, sample_format }` | `TtsPlayer { sink: MixerDeviceSink }` |
| `detect_sample_format` 试开流探测 | `DeviceSinkBuilder::from_device(d).open_sink_or_fallback()` 内置回退 |
| `play()`: f32/i16 双回调 + 计数等待 | `Player::append(SamplesBuffer)` + `sleep_until_end()` |
| `start_streaming()`: 共享 buffer + drain 回调 | `queue::queue(true)`, `write_chunk` → `tx.append(SamplesBuffer)` |
| `mark_synthesis_done` 仅置标志 | 置标志 + `tx.set_keep_alive_if_empty(false)` |
| `is_done` = 回调置位 | `synthesis_done && player.empty()` |

**完成语义等价性** (spec FR-004):
- 非流式: `sleep_until_end()` 阻塞到队列清空, 等价于现状"等全部样本写完".
- 流式: `keep_alive_if_empty=true` 防止欠载时队列提前结束; 合成完成时
  关掉 keep-alive, 队列播空后 `player.empty()` 为真 → `is_done`.
  合成线程报错路径不变 (`synthesis_done` 照常置位, 已合成部分播完收尾).

**设备句柄语义变化** (需文档化): 输出流从"每次播放开关"变为"`TtsPlayer`
存活期常驻"。`SharedState::rebuild_voice` 切设备时 drop 旧 `TtsPlayer` →
`MixerDeviceSink` Drop 释放设备, 原 RAII 链保持。`OwnedOutputStream`
删除前 grep 确认无其他使用方。

## Complexity Tracking

> **Fill ONLY if Constitution Check has violations that must be justified**

无违规项。
