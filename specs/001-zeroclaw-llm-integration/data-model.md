# Data Model: LLM 模块接入 ZeroClaw

**Date**: 2026-08-02 | **Feature**: [spec.md](spec.md)

## 本仓库侧实体

### ZeroclawLlm（新增，`llm/zeroclaw.rs`）

实现 `LlmTrait` 的适配器，接管 `chat()`。

| 字段 | 类型 | 说明 |
|------|------|------|
| acp | AcpClient | 持有 zeroclaw 子进程的 ACP 连接 |
| session_id | Option\<String\> | 当前 ACP session；None 时下次 chat 惰性 `session/new` |
| timeout | Duration | 单轮回复超时（默认 ≤5s，超时即判定不可用） |

**不变量**：自身不保存任何对话历史；历史完全由 zeroclaw 侧 session 持有（FR-002）。

### AcpClient（新增，`llm/acp.rs`）

最小 JSON-RPC 2.0 stdio 客户端。

| 字段 | 类型 | 说明 |
|------|------|------|
| child | tokio Child | `zeroclaw acp` 子进程句柄 |
| next_id | u64 | JSON-RPC 请求 id 自增 |
| state | AcpState | 状态机：New → Initialized → Ready；任一 IO 错误 → Broken |

**状态转移**：
- `New` --initialize 成功--> `Initialized`
- `Initialized` --session/new 成功--> `Ready`（可 prompt）
- 任意状态 --IO 错误/子进程退出--> `Broken` → 下次 chat 时整体重建（杀进程、重 spawn、重走 New→Ready）

### 对话回复流（chat 一轮）

```
ASR 文本 -> ZeroclawLlm.chat
  -> ACP session/prompt (用户文本)
  -> 聚合流式 notification 直到 completion
  -> String (回复文本) -> TTS 播报
错误/超时 -> Err -> 语音链路播报"服务不可用"提示 (FR-004, US3)
```

## ZeroClaw 侧实体（外部持有，本仓库只读写接口）

### Conversation Session（zeroclaw sessions.db）

- 标识：ACP `session/new` 返回的 session id
- 生命周期：`session/new` 创建 → 多轮 `session/prompt` → `session/stop` 或子进程重启销毁
- 清空记忆时：旧 session `session/stop` + `session/new` 重建（历史随旧 session 废弃）

### Memory（zeroclaw brain.db + USER.md）

- 由 zeroclaw `memory.auto_save` 自动从对话抽取持久化
- 清空入口：proto 新命令 `ClearLlmMemory` → server 执行 `zeroclaw memory clear --yes`
- 本仓库不直接读写 brain.db

## 配置映射（AppConfig → zeroclaw config.toml）

| 本仓库 AppConfig | zeroclaw 配置 | 说明 |
|------------------|---------------|------|
| llm_api_key | providers.models.doubao.ark.api_key | 渲染时写入 |
| llm_model | providers.models.doubao.ark.model | 渲染时写入 |
| llm_api_base | providers.models.doubao.ark.uri | endpoint 覆盖 |
| （固定人设） | agents/robot/workspace/SOUL.md | 静态模板下发 |

校验规则：三项任一为空 → 不启用 zeroclaw 链路（启动日志明确提示，chat 直接走不可用播报）。

## 既有实体变更

- `LlmTrait`：`set_session_id` / `clear_session_history` / `clear_all_histories` 随历史自管理逻辑一起移除或标记废弃（FR-002/SC-003）；`analyze_mood` 与 `chat` 签名不变
- `LlmResponse`（mood/actions）：不变，analyze_mood 链路不受影响
- proto `Command`：新增 `ClearLlmMemory` 可选变体（只增不改）
