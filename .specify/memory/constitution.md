<!--
Sync Impact Report
- Version change: 1.0.0 → 1.1.0 (MINOR: 原则 V 实质性扩充, 新增"更优方案 MUST 上报"条款)
- Modified principles: V. 最小改动与简洁 → 同名, 增加例外条款
- Added sections: 无
- Removed sections: 无
- Follow-up TODOs: 无
-->
# ElectronBotCli 宪法

本文件是项目的最高治理约束，优先级高于任何临时实践与个人偏好。
日常协作细则见 `CLAUDE.md`，二者冲突时以本宪法为准。

## Core Principles

### I. 输入派发契约不可绕过

TUI 的所有输入必须经过 `crates/ele_bot_client/src/input/mod.rs` 的
`handle_event` / `handle_by_mode` 统一派发，**禁止绕过它直接调用 `App` 方法**。
派发契约以 `handle_event` 的 rustdoc 为唯一权威定义；修改输入相关代码前必须先读
该契约。新增输入模式时，优先在现有派发层内扩展，而不是新建旁路。

### II. 文档即接口（rustdoc 强制）

任何 `pub` 函数、结构体、枚举、trait 必须有 `///` 文档注释，至少包含：一句话职责、
边界/不变量；推荐附 `# Examples`（用 ` ```rust,ignore `，不跑 doctest）。
每个 `.rs` 文件顶部用 `//!` 写 2-4 行模块职责说明。
行内注释一律中文，解释**为什么**而非**做什么**；显而易见的注释不写。
公共 API 缺少合格 rustdoc 的代码视为未完成。

### III. 质量门禁（NON-NEGOTIABLE）

提交前必须依次通过以下三项，任何一项失败都不提交：

```bash
cargo fmt --all
cargo clippy --all-features --all-targets -- -D warnings
cargo check --all-features --all-targets
```

CI（`.github/workflows/ci.yml`）执行 fmt / clippy / build / test，本地门禁是 CI 的
前置防线，不允许"先提交让 CI 看看"。

### IV. 协议双向兼容

`ele_bot_proto` 是 client 与 server 之间唯一的协议契约。client 与 server 部署
解耦（服务端常驻 RK3566），版本可能不同步，因此：

- 协议消息**只增不改**：新增字段/变体须可选或有默认值，不得改变既有字段语义。
- 破坏性改动（删除/改义字段、改枚举语义）必须保证新旧版本 client/server 交叉
  工作，且在 spec 与 commit 信息中显式标注。
- 无法兼容的协议演进，须先在 spec 中给出迁移方案，经确认后才可实现。

### V. 最小改动与简洁

改动只覆盖任务实际涉及的范围：不顺手重构、不臆测性抽象、不引入未经验证的依赖。
三行相似代码好过一次过早抽象。新增第三方 crate 前，先确认 workspace 内没有
现成能力，并优先复用已有依赖的版本与惯用法。

**例外（ MUST 上报）**：若执行中发现存在明显更优的依赖或方案（官方 SDK、更小的
实现成本、显著更好的可维护性），**必须先向用户说明权衡并征得同意**，再决定引入
或替换；不得仅以"最小依赖"为由默默排除更优解，也不得未经确认擅自引入。

## 技术栈与平台约束

- **Workspace 结构**：`ele_bot_proto`（共享协议类型）、`ele_bot_server`（串口舵机、
  摄像头、ASR/TTS、LLM）、`ele_bot_client`（ratatui TUI，WebSocket 通信）。
  共享类型只能放 proto，禁止 client/server 各自定义协议结构。
- **目标平台**：开发机 Windows；服务端部署目标为 RK3566（aarch64-unknown-linux-gnu，
  交叉编译见 `Cross.toml` / `Dockerfile.cross`）。涉及硬件、音频、路径的代码必须
  考虑跨平台，禁止写死 Windows 专属假设进 server 端。
- **语言与文本**：对话、行内注释、commit 信息、`log::*!` 日志一律中文；UI 字符串
  中文为主、英文术语保留；rustdoc 与代码标识符保持现有风格。
- **测试**：关键模块必须带测试 —— 包括 proto 协议序列化、input 派发逻辑、以及任何
  纯逻辑的修复（回归测试）。TUI 页面渲染、硬件交互代码不强制，但鼓励可测部分抽离。

## 开发流程

- **修改前先读**：任何文件先用 Read 看完整内容再动，不基于猜测改。
- **任务颗粒度**：超过 3 步的改动用任务清单跟踪，逐项标记完成。
- **Spec 驱动**：新功能、协议变更、跨 crate 的行为修改，先走 openspec
  （proposal → design → tasks）或 speckit 流水线，再写代码；spec 产物提交入库。
- **Commit 信息**：首行 `[<类别>/] <中文短句>`，类别常用 `新增` `修复` `发布` `优化`，
  首行 ≤ 50 字，不加 emoji；修复类提交在正文追加报错日志原文。不使用英文
  `feat:` / `fix:` 前缀。
- **AI 代理纪律**：不执行未获明确许可的 git 变更操作（commit/push/reset 等）；
  破坏性操作前先确认。

## Governance

- 本宪法优先于其它实践；与 `CLAUDE.md` 冲突时以本宪法为准，`CLAUDE.md` 作为日常
  开发指导文件继续使用。
- 修订须满足：明确动机、在 commit 中说明、同步更新 `CLAUDE.md` 等受影响文档。
- 所有 PR 与 AI 代理产出必须对照本宪法自查；复杂度超出现有模式的部分须说明理由。
- 版本号规则：不兼容的原则删除/改义升 MAJOR，新增原则或章节升 MINOR，措辞澄清
  升 PATCH。

**Version**: 1.1.0 | **Ratified**: 2026-07-30 | **Last Amended**: 2026-08-06
