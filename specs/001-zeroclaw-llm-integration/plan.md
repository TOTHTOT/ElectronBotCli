# Implementation Plan: LLM 模块接入 ZeroClaw 托管对话与记忆

**Branch**: `001-zeroclaw-llm-integration` | **Date**: 2026-08-02 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `/specs/001-zeroclaw-llm-integration/spec.md`

## Summary

把语音对话链路（ASR → LLM → TTS）中的 LLM 对话回复与对话历史/用户记忆托管给设备端 ZeroClaw：`ele_bot_server` spawn `zeroclaw acp` 子进程，通过 ACP（JSON-RPC 2.0 over stdio）转发用户文本并取回回复；本仓库移除自管理的 session 历史累积/清除逻辑。analyze_mood（表情/舵机）保留现有在线/本地 LLM 链路不动。ZeroClaw 不可用时不回退，超时（≤5s）播报"服务不可用"提示。

## Technical Context

**Language/Version**: Rust（workspace 现有 toolchain，见 `rust-toolchain`/CI），tokio async

**Primary Dependencies**: 复用 workspace 已有 `tokio`（process/stdio）、`serde`/`serde_json`、`async-trait`、`anyhow`、`log`；**不新增重型依赖**——ACP 客户端用 tokio + serde_json 实现最小 JSON-RPC stdio 交互（宪法 V）。外部进程：ZeroClaw 官方 `aarch64-unknown-linux-musl` 静态二进制（锁定版本）

**Storage**: 本仓库无新增存储；对话历史与记忆由 ZeroClaw SQLite 持有（`<config-dir>/data/sessions/sessions.db`、`data/memory/brain.db`）

**Testing**: `cargo test`（ACP 客户端帧解析/session 状态机单测；mock 子进程联调）；设备端 quickstart 手动验证

**Target Platform**: RK3566（aarch64-unknown-linux-gnu，Debian 11，glibc 2.31，~2GB RAM）；开发机 macOS 可联调（zeroclaw 有 darwin 版）

**Project Type**: 嵌入式语音机器人 server（Rust workspace，TUI client 经 WebSocket 控制）

**Performance Goals**: 单轮对话（ASR 结束 → TTS 开始）延迟劣化 ≤20%；zeroclaw 故障 ≤5s 给出语音反馈

**Constraints**: 内存受限（zeroclaw 禁用 embeddings/多余 channels）；RK3566 无构建能力，一律预编译产物下发；协议只增不改（宪法 IV）

**Scale/Scope**: 单用户单设备；改动集中在 `ele_bot_server` llm 模块 + proto 新增 1 个命令变体 + client TUI 1 个入口 + 部署脚本

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| 原则 | 评估 | 结果 |
|------|------|------|
| I. 输入派发契约 | TUI 新增"清空记忆"入口走 `input/mod.rs` 统一派发，不旁路 | PASS |
| II. rustdoc 强制 | 新增 pub 项（AcpClient/ZeroclawLlm 等）按标准写 rustdoc | PASS（实现时执行） |
| III. 质量门禁 | fmt/clippy/check 三件套，提交前必过 | PASS（实现时执行） |
| IV. 协议双向兼容 | proto 仅**新增**可选命令变体（清空记忆），不改既有字段语义 | PASS |
| V. 最小改动 | ACP 客户端用现有 tokio/serde_json 实现，不引新 crate；analyze_mood 不动 | PASS |
| 测试要求 | ACP 帧解析与 session 状态机为纯逻辑，必带单测 | PASS（实现时执行） |
| 部署约束 | zeroclaw musl 静态二进制随 deploy 脚本下发，避开 glibc 2.31 风险 | PASS |

无违规，无需 Complexity Tracking。

## Project Structure

### Documentation (this feature)

```text
specs/001-zeroclaw-llm-integration/
├── plan.md              # This file
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── contracts/           # Phase 1 output
│   ├── zeroclaw-acp.md
│   └── zeroclaw-config.md
└── tasks.md             # Phase 2 output (/skill:speckit-tasks)
```

### Source Code (repository root)

```text
crates/
├── ele_bot_proto/src/
│   └── (Command 枚举新增 ClearLlmMemory 可选变体 —— 只增不改)
├── ele_bot_server/src/
│   ├── llm/
│   │   ├── mod.rs           # LlmManager 接入 ZeroclawLlm (chat 走 zeroclaw)
│   │   ├── trait_.rs        # LlmTrait: 移除/停用 session 历史方法 (FR-002)
│   │   ├── zeroclaw.rs      # 新增: ZeroclawLlm 实现 LlmTrait.chat
│   │   └── acp.rs           # 新增: 最小 ACP (JSON-RPC/stdio) 客户端
│   ├── (zeroclaw 配置渲染: 由 config/AppConfig -> zeroclaw config.toml/SOUL.md)
│   └── media/voice/         # 语音链路 chat 调用处: 加 5s 超时与"服务不可用"播报
└── ele_bot_client/src/
    └── (TUI 新增"清空对话记忆"入口, 走 input/mod.rs 统一派发)
scripts/
└── deploy_rk3566.sh       # 新增 zeroclaw musl 二进制 + 配置下发
assets/
└── zeroclaw/              # zeroclaw 二进制与 config 模板 (SOUL.md 人设)
```

**Structure Decision**: 沿用现有 workspace 三 crate 结构；zeroclaw 集成代码全部收在 `ele_bot_server/src/llm/` 内（`zeroclaw.rs` 业务适配 + `acp.rs` 协议客户端分离），对外只暴露 `LlmTrait` 不变接口；proto/client 各一处只增式扩展。
