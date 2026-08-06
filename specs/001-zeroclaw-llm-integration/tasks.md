---

description: "Task list for LLM 模块接入 ZeroClaw 托管对话与记忆"

---

# Tasks: LLM 模块接入 ZeroClaw 托管对话与记忆

**Input**: Design documents from `/specs/001-zeroclaw-llm-integration/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/, quickstart.md

**Tests**: 宪法要求纯逻辑（ACP 帧编解码/session 状态机）必带单测；spec 未要求全面 TDD，其余以设备端 quickstart 场景验证。

**Organization**: 任务按用户故事分组，每个故事可独立实现与验证。

## Format: `[ID] [P?] [Story] Description`

- **[P]**: 可并行（不同文件、无未完成依赖）
- **[Story]**: [US1]/[US2]/[US3] 对应 spec.md 用户故事
- 描述含确切文件路径

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: zeroclaw 二进制、配置模板与部署通道就绪

- [X] T001 下载 zeroclaw 官方 `aarch64-unknown-linux-musl` 静态二进制（锁定版本，记录 sha256）到 `assets/zeroclaw/zeroclaw`；macOS 联调副本放 `target/zeroclaw-spike/`（已有 v0.8.3，确认版本一致或升级）
- [X] T002 [P] 创建 `assets/zeroclaw/zc-config/` 模板：`config.toml`（doubao provider 占位 + robot agent + 关闭多余能力，按 contracts/zeroclaw-config.md）与 `agents/robot/workspace/SOUL.md`（机器人固定人设）
- [X] T003 [P] `scripts/deploy_rk3566.sh` 新增下发 `assets/zeroclaw/zeroclaw` → 设备 `~/ElectronBotCli/zeroclaw`（含 chmod +x）与 zc-config 模板（设备已有数据目录时不覆盖 `data/`）

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: ACP 客户端就绪——所有故事都依赖它与 zeroclaw 通信

**⚠️ CRITICAL**: 本阶段未完成前不做任何用户故事

- [X] T004 实现最小 ACP 客户端 `crates/ele_bot_server/src/llm/acp.rs`：tokio spawn `zeroclaw acp` 子进程、JSON-RPC 2.0 帧编解码、状态机 New→Initialized→Ready（`initialize`/`session/new`/`session/prompt` 聚合流式 notification/`session/stop`）、IO 错误→Broken。只用 workspace 已有 tokio + serde_json，不引新 crate（宪法 V）
- [X] T005 为 `acp.rs` 写单测（同文件 `#[cfg(test)]`）：帧解析（完整/分片/多对象）、请求 id 匹配、状态机转移与 Broken 重建逻辑；用 mock stdin/stdout 不依赖真实 zeroclaw（宪法：纯逻辑必带测试）
- [X] T006 macOS 本地联调：用 `target/zeroclaw-spike/zeroclaw acp` 实测真实帧格式与 notification 结构（可用 `agent -m` 之外的手动 JSON-RPC echo 验证），如有出入修正 `specs/001-zeroclaw-llm-integration/contracts/zeroclaw-acp.md` 与 T004 实现；确认 doubao ark 端点实跑通（research 风险 3）

**Checkpoint**: ACP 客户端本地能完成 initialize→session/new→prompt→收回复全链路

---

## Phase 3: User Story 1 - 多轮语音对话上下文自动延续 (Priority: P1) 🎯 MVP

**Goal**: chat 改走 zeroclaw，历史由 zeroclaw 托管，本仓库移除自管理历史

**Independent Test**: quickstart 场景 1——连续 3 轮含指代对话上下文正确，代码审查确认历史累积逻辑已移除（SC-001/SC-003）

### Implementation for User Story 1

- [X] T007 [US1] 实现 `crates/ele_bot_server/src/llm/zeroclaw.rs`：`ZeroclawLlm` 持有 `AcpClient`，session 惰性创建（None 时先 `session/new`），`chat()` 聚合流式回复为 String；自身不保存任何历史（data-model.md 不变量）
- [X] T008 [P] [US1] server 启动时把 `AppConfig.llm_api_base/api_key/model` 渲染进设备侧 `zc-config/config.toml`（`crates/ele_bot_server/src/config.rs` 或 state 启动路径）；三项任一为空则不启用 zeroclaw 并日志提示（contracts/zeroclaw-config.md 前置条件）
- [X] T009 [US1] 改造 `crates/ele_bot_server/src/llm/mod.rs` `LlmManager`：拆两个后端——`chat()` 走 `ZeroclawLlm`，`analyze_mood()` 保留现有 OnlineLlm/QwenLlm（spec Q2）；`state.rs:241` 构造处同步调整
- [X] T010 [US1] 移除历史自管理：`crates/ele_bot_server/src/llm/trait_.rs` 删除 `set_session_id`/`clear_session_history`/`clear_all_histories`，`online.rs`（及 `qwen.rs`、`mod.rs` 转发层）移除对应实现与历史累积逻辑（FR-002/SC-003）
- [X] T011 [US1] 交叉编译部署到 RK3566，跑 quickstart 场景 1（3 轮指代对话 + 日志确认 zeroclaw spawn/session 成功）。实测：设备端（gemini-2.5-flash）与 Mac 本地（用户自配 minimax + xiaobo 人设）均通过——轮2 正确答出"小明/8岁"

**Checkpoint**: US1 独立可用——MVP 达成

---

## Phase 4: User Story 2 - 用户个人信息长期记忆 (Priority: P2)

**Goal**: 记忆跨重启保留（zeroclaw auto_save 开箱即用）+ 提供整体清空入口

**Independent Test**: quickstart 场景 2、3——重启后仍记得个人信息；TUI 清空后不再记得且对话可继续

### Implementation for User Story 2

- [X] T012 [P] [US2] `crates/ele_bot_proto/src/messages.rs` `ClientMessage` 新增 `ClearLlmMemory` 变体（只增不改，宪法 IV；serde tag 协议下旧端收到未知变体会报错——处理见 T013 注释）
- [X] T013 [US2] `crates/ele_bot_server/src/state.rs` 命令处理分支实现 `ClearLlmMemory`：ACP `session/stop` → `zeroclaw --config-dir zc-config memory clear --yes` → `session/new` 重建（contracts/zeroclaw-config.md 命令链）
- [X] T014 [US2] client TUI 新增"清空对话记忆"入口：`crates/ele_bot_client/src/input/mod.rs` 统一派发（宪法 I）+ `crates/ele_bot_client/src/app/mod.rs` 发送 `ClientMessage::ClearLlmMemory`（参照 `app/mod.rs:419` SetConfig 模式）
- [X] T015 [US2] 设备端跑 quickstart 场景 2、3（重启记忆保留 / 清空后遗忘且 `memory stats` 归零）。注：设备离线期间改为 Mac 本地（用户自配 zeroclaw）验证，两项均通过；设备回来后补跑

**Checkpoint**: US1、US2 均独立可用

---

## Phase 5: User Story 3 - ZeroClaw 不可用时的行为 (Priority: P3)

**Goal**: 故障时不卡死、≤5s 播报"服务不可用"，恢复后自动继续

**Independent Test**: quickstart 场景 4——`chmod -x zeroclaw` 后对话得到语音提示，恢复后自动可用

### Implementation for User Story 3

- [X] T016 [US3] `crates/ele_bot_server/src/state.rs:505-515` chat 调用包 5s `tokio::time::timeout`，失败文案改为固定的"服务不可用"类用户友好提示（替换当前 `[LLM 错误: {e}]` 原文外露），错误细节进日志（SC-005）
- [X] T017 [US3] `zeroclaw.rs`/`acp.rs` 实现 Broken 自动恢复：检测到子进程退出后，下次 `chat()` 自动重 spawn + initialize + session/new（data-model.md 状态转移）
- [X] T018 [P] [US3] 启动前置检查落地：llm 配置三项任一为空 → 不 spawn zeroclaw、启动日志明确提示、chat 一律走不可用播报（衔接 T008）
- [ ] T019 [US3] 设备端跑 quickstart 场景 4（含恢复验证）

**Checkpoint**: 三个故事全部独立可用

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: 指标验证与收尾

- [ ] T020 设备端跑 quickstart 场景 5：ASR 结束→TTS 开始延迟对比接入前基线（劣化 ≤20%，SC-004）；`ps -o rss= -C zeroclaw` 记录 RSS 确认 2GB 设备可接受
- [X] T021 [P] 宪法 III 质量门禁：`cargo fmt --all`、`cargo clippy --all-features --all-targets -- -D warnings`、`cargo check --all-features --all-targets`、`cargo test -p ele_bot_server`
- [ ] T022 [P] 更新 `CLAUDE.md` 与相关 docs：LLM 链路改为 zeroclaw 托管的说明、部署前置（zeroclaw 二进制）、联调命令（`zeroclaw doctor`/`memory stats`）
- [X] T023 整理提交（spec 产物 + 实现；commit 信息按宪法格式，执行前需用户确认）。提交 `7a4b93f`，fmt/clippy -D warnings/44 测试全绿

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: 无依赖，T001/T002/T003 可并行
- **Foundational (Phase 2)**: 依赖 T001（需要 zeroclaw 二进制做联调）；**阻塞所有用户故事**
- **User Stories (Phase 3-5)**: 均依赖 Phase 2 完成；US2/US3 的实现任务依赖 US1 的 `ZeroclawLlm` 存在，但验证场景各自独立
- **Polish (Phase 6)**: 依赖 US1-US3 完成

### User Story Dependencies

- **US1 (P1)**: Phase 2 完成后即可开始，无其它故事依赖
- **US2 (P2)**: 代码上依赖 T007/T009（chat 链路已走 zeroclaw）；T012 proto 变更可与 US1 并行
- **US3 (P3)**: 依赖 T007（ZeroclawLlm）；T016/T018 彼此独立

### Parallel Opportunities

- T002、T003 并行（不同文件）
- T008 与 T007 并行（配置渲染 vs ACP 适配器）
- T012（proto）与整个 US1 并行
- T018 与 T016/T017 并行

## Parallel Example: Phase 1

```bash
Task: "创建 assets/zeroclaw/zc-config/ 模板 (T002)"
Task: "deploy_rk3566.sh 新增 zeroclaw 下发 (T003)"
```

## Parallel Example: User Story 1

```bash
Task: "实现 llm/zeroclaw.rs ZeroclawLlm (T007)"
Task: "AppConfig 渲染 zc-config/config.toml (T008)"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. 完成 Phase 1 + Phase 2（ACP 客户端本地联调通过）
2. 完成 Phase 3（US1）
3. **STOP and VALIDATE**: 设备端 quickstart 场景 1
4. 可用即演示 MVP：对话历史已托管 zeroclaw

### Incremental Delivery

1. Setup + Foundational → 通信底座就绪
2. +US1 → 设备验证 → MVP
3. +US2 → 设备验证（记忆 + 清空入口）
4. +US3 → 设备验证（降级播报）
5. Polish → 指标确认 + 门禁 + 提交

---

## Notes

- T006 是关键风险解除点：zeroclaw acp 真实帧格式与 doubao ark 端点若不通，后续故事全部受阻，务必优先做
- 所有 pub 项补 rustdoc（宪法 II）；日志/注释/commit 用中文
- `zeroclaw` 配置里的真实 api_key 不要提交进 git（config.toml 模板用占位符，运行时渲染）

## 设计变更记录 (2026-08-06)

- **zeroclaw 配置改为用户自管理**: 废弃 T002/T003/T008 的模板渲染方案（`assets/zeroclaw/zc-config/` 已删除，deploy 只下发二进制）。server spawn 时不传 `--config-dir`，provider/api_key/人设完全由用户在设备/本机自行配置（homebrew 版配置在 `/opt/homebrew/var/zeroclaw/config.toml`）；AppConfig 的 `llm_*` 只喂 `analyze_mood`。合约文档已同步更新
- **prompt 失败自动重试一次**: 修复 state.rs 60s 播报超时提前取消 prompt future 导致 session 楔死（"Session already has an active prompt turn"）的问题——`chat()` 出错后杀子进程整体重建并重试一次，对调用方透明
- **模型**: 用户改用 MiniMax-M2.5-highspeed 后，记忆写入轮从 60s+ 降到 ~8s，普通轮 2-10s，state.rs 60s 上限不再是问题（旧记录：thinking 模型记忆轮 47-60s+ 会触发 60s 播报上限）
- **session cwd 必须命中 agent workspace**: 实测 cwd 不匹配时 zeroclaw 不注入 SOUL.md/MEMORY.md，人设与长期记忆全失效。`session_workspace()` 自动探测 `~/.zeroclaw` 与 homebrew `/opt/homebrew/var/zeroclaw` 下的 `agents/*/workspace`，`ZCLAW_WORKSPACE` 可覆盖
- **Mac 本地全场景验证通过**（用户自配 minimax + xiaobo）: 多轮上下文（小明/8岁 ✓）、跨重启记忆（小白 🐱 ✓）、清空记忆（✓）、故障 2s 降级播报（✓）
- **ACP 工具审批**: zeroclaw 在 ACP 模式把工具审批委托给客户端（`session/request_permission`），不回应会导致记忆写入轮永久挂起。`acp.rs` 自动回 allow-once（不固化永久规则）；可用工具与高危拦截由用户 zeroclaw 配置/risk_profile 决定，本仓库不越权否决
