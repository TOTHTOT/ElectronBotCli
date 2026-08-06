# Phase 0 Research: LLM 模块接入 ZeroClaw

**Date**: 2026-08-02 | **Feature**: [spec.md](spec.md)

调研方式：本地 spike 二进制实测（`target/zeroclaw-spike/zeroclaw` v0.8.3 `--help`/子命令）+ 官方 GitHub/文档。

## Decision 1: 本仓库 ↔ ZeroClaw 的交互方式 → ACP（JSON-RPC 2.0 over stdio）

- **Decision**: `ele_bot_server` spawn 并持有 `zeroclaw acp` 子进程，走标准 Agent Client Protocol：`initialize` → `session/new` → `session/prompt`（流式 notification 收回复）→ `session/stop`。子进程崩溃则重启重建 session。
- **Rationale**: 无端口、无 pairing 鉴权管理；ACP 是外部开放标准（agentclientprotocol.com），受 ZeroClaw 自身快速迭代的冲击最小；会话语义与"对话托管"模型完全吻合。
- **Alternatives considered**:
  - `zeroclaw daemon` + 本地 HTTP/WebSocket（次选/备选）：进程解耦更彻底、`GET /api/doctor` 健康检查现成，但需处理 pairing token（`gateway get-paircode`）与端口占用，REST 端点无版本化承诺。
  - 一次性 CLI `zeroclaw agent -a robot -m "文本"`：每轮冷启动 + 自管 `--session-state-file`，丢掉常驻会话/记忆优势；仅作为联调与降级诊断手段。

## Decision 2: 设备端部署形态 → 官方 musl 静态二进制 + systemd user service

- **Decision**: 使用官方 release 的 `zeroclaw-aarch64-unknown-linux-musl.tar.gz` 预编译二进制；部署脚本随 `ele_bot_server` 一起下发；`zeroclaw service install` 注册 systemd user service（仅当采用 daemon 形态时；ACP 形态由 ele_bot_server 直接 spawn，无需独立服务）。
- **Rationale**: RK3566 为 Debian 11 / glibc 2.31，gnu 版构建机 glibc 下限未标明有兼容风险；musl 静态版彻底回避，与本仓库 musl target 经验一致。板上构建不现实（官方要求 2GB+ RAM、6-10GB 磁盘）。
- **Alternatives considered**: gnu 版（赌 glibc 兼容，不取）；源码交叉编译（维护成本高，仅预编译不可用时兜底）。

## Decision 3: LLM Provider 配置 → zeroclaw 原生 doubao slot，复用现有凭据

- **Decision**: zeroclaw `config.toml` 配置 `[providers.models.doubao.ark]`（api_key/model/uri），agent `robot` 引用之。本仓库现有 `llm_api_base/api_key/model` 配置在部署/初始化时渲染进 zeroclaw 配置，用户不维护两份。
- **Rationale**: 调研确认 zeroclaw 有 doubao/Volcengine 原生 provider slot，支持 `uri` 覆盖自定义 OpenAI 兼容 endpoint、`timeout_secs`、`fallback`。spike 的 `zc-config/config.toml` 已通过 `agents list` 校验。
- **Alternatives considered**: 自定义 OpenAI 兼容 provider（doubao slot 已原生支持，多余）。
- **未验证风险**: ark 的 `/chat/completions` 路径与 `wire_api` 默认值未实跑（避免消耗 key 额度），首次联调须验证，必要时 `uri` 显式覆盖。

## Decision 4: 记忆与历史存储 → zeroclaw 默认 SQLite，禁用 embedding 向量检索

- **Decision**: 对话历史 = `<config-dir>/data/sessions/sessions.db`；长期记忆 = `<config-dir>/data/memory/brain.db`（sqlite backend，auto_save 开启）。`embedding_provider = none`（默认关键词/BM25 检索，不产生额外 API 调用、内存最省）。人设写入 `agents/robot/workspace/SOUL.md`，用户信息自动沉淀到 `USER.md`/memory。
- **Rationale**: 2GB 内存的 RK3566 上禁用 embeddings 最稳；官方宣称运行时 <5MB RAM 为厂商口径，需实测 RSS。
- **Alternatives considered**: 开启 embedding 向量检索（检索质量更好但增加内存/依赖，留作后续优化）。

## Decision 5: 清空历史/记忆的实现 → `zeroclaw memory clear` + 删除 session

- **Decision**: TUI 新增"清空对话记忆"入口 → proto 新增**可选变体**命令（只增不改，符合宪法 IV）→ server 侧执行 `zeroclaw memory clear --yes` 并重建 ACP session（旧 session `session/stop` + `session/new`）。
- **Rationale**: 本期只做整体清空（spec Q3 用户确认）；`memory clear` 支持 `--category`/`--key`，后续要精细删除可直接扩展。
- **Alternatives considered**: 直接删 `brain.db`/`sessions.db` 文件（粗暴，需停进程，不如官方命令）。

## Decision 6: analyze_mood 保留现有链路

- **Decision**: 情感/动作分析（表情 + 舵机）不迁移，继续走现有 `OnlineLlm`/`QwenLlm`。ZeroClaw 只接管 `chat()` 对话回复与历史/记忆。
- **Rationale**: spec Q2 用户确认；结构化输出（mood + actions JSON）在 ACP 上无现成约定，迁移成本高收益低。

## 关键事实备查

- ZeroClaw：Rust 编写的自托管 AI agent 运行时（OpenClaw 的轻量替代），MIT/Apache-2.0，github.com/zeroclaw-labs/zeroclaw，迭代极快（v0.8.3 → v0.8.4 有 Breaking Changes）→ **锁定版本**，升级前读 changelog。
- ACP 子命令实测：`zeroclaw acp [--max-sessions N] [--session-timeout SECS]`，methods: `initialize, session/new, session/prompt, session/stop`；v0.8.4 起 `session/new` 支持 `?agent=`。
- 健康检查：`zeroclaw status --format exit-code`、`zeroclaw doctor`（联调用）。
- provider 失败行为：`timeout_secs` + `fallback`/`fallback_models` 链；spec 要求的"≤5 秒播报服务不可用"由本仓库侧超时控制兜底（ACP 请求加 timeout）。
- 配置 schema 迁移不总无痛（`config migrate` 修复）→ 部署时固定由脚本渲染配置，不让 zeroclaw 自动迁移手写配置。

## 风险清单

| 风险 | 等级 | 缓解 |
|------|------|------|
| zeroclaw 版本 Breaking Changes | 中 | 锁定版本；ACP 标准协议缓冲；升级前读 changelog |
| RK3566 内存占用未实测 | 中 | 禁用 embeddings/channels/tools；quickstart 含 RSS 测量 |
| doubao ark 端点未实跑 | 中 | 联调第一步验证；`uri` 显式覆盖兜底 |
| 历史重复注入（本仓库旧逻辑未清） | 高 | FR-002：接入同时移除 session 历史累积/清除代码 |
| ACP Rust 客户端需自实现 | 低 | 用 workspace 已有 tokio + serde_json 实现最小 JSON-RPC stdio 客户端，不引新重依赖（宪法 V） |
