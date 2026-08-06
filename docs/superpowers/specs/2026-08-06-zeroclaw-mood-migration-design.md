# zeroclaw 托管 mood 分析 — 移除 OnlineLlm/QwenLlm 双后端

日期: 2026-08-06
状态: 已批准 (用户确认 2026-08-06)
推翻: specs/001-zeroclaw-llm-integration 的 Q2 决策 ("analyze_mood 不迁移, 保留在线/本地链路")

## 动机

`LlmManager` 双后端中, mood 链路的 `OnlineLlm` (async-openai) 与 `QwenLlm` (Candle GGUF)
只占内存和编译体积, 价值低: 对话已全走 zeroclaw, mood 分析只是一次短指令调用,
zeroclaw 完全能胜任。全部交给 zeroclaw 后:

- 删除 async-openai / candle 依赖, 编译体积与内存占用下降
- 删掉 config.toml 的 api_base/api_key/model/model_path/tokenizer_path 五项配置
- LLM 供应商只剩一处 (zeroclaw 用户自管配置), 心智负担最低

## 架构

`LlmManager` 单后端化: 只持有 `Arc<Mutex<ZeroclawLlm>>`, `new()` 无参。

- `chat(input)` → 现状不变 (chat session, 60s 超时, 失败降级文案)
- `analyze_mood(input)` → zeroclaw **独立 mood session**, 失败回退 `LlmResponse::default()`
  (中性 + 无动作, state.rs 已有此兜底)
- `clear_llm_memory()` → 现状不变, 只清 chat 侧

## mood session 方案

一个 `zeroclaw acp` 进程挂两个 session (ACP `session/prompt` 自带 sessionId, 协议原生支持):

- `AcpClient` 从单 `session_id` 扩展为多 session: `session_new` 返回 id 由调用方持有,
  `prompt(session_id, text, timeout)` 显式传入; `session_stop(session_id)` 按 id 停止。
- chat session: 现状不变, `ZeroclawLlm` 长期持有。
- mood session: **每次分析新建** (`session/new` → `prompt` → `session/stop`),
  保证每轮分析都是干净上下文, token 不随历史滚雪球; 同时不污染 xiaobo 的对话历史与记忆。
- mood prompt 为自包含单条消息: 现有 `system_prompt()` 指令文本 ([情感] 标签 +
  舵机动作 JSON 格式说明) + `用户输入：{input}`。SOUL.md 人设可能干扰格式,
  解析失败回退 `[中性]`, 风险可控。

## 代码变更

1. `llm/online.rs` 的 `system_prompt()` / `split_response` / `parse_mood` / `parse_actions`
   四个纯函数 (含单测) 迁到 `llm/response.rs` (与 `LlmResponse`/`Mood`/`Action` 同文件)。
2. `llm/acp.rs`: 多 session 化 (见上)。
3. `llm/zeroclaw.rs`: 新增 `analyze_mood(&mut self, user_input) -> Result<LlmResponse>`,
   内部 session/new → prompt (复用 PROMPT_TIMEOUT) → session/stop → 解析;
   任何一步出错走现有 drop_client 重建语义。
4. `llm/mod.rs`: `LlmManager { llm: Arc<Mutex<ZeroclawLlm>> }`, `new()` 无参,
   `analyze_mood` 委托 zeroclaw; 删 `check_network` / `create_local_llm`。
5. 删除 `llm/online.rs` / `llm/qwen.rs` / `llm/trait_.rs`。
6. `Cargo.toml` (ele_bot_server): 移除 `async-openai`, `candle-core`, `candle-nn`,
   `candle-transformers`, `tokenizers` (按实际依赖树核实)。
7. `config.rs` / `config.toml`: 移除 llm 段五个字段; 旧配置文件多出字段 serde 默认忽略, 兼容。
8. `state.rs`: `LlmManager::new(...)` 调用点改无参; `spawn_llm_thread` 业务逻辑不变。

## 不做 (YAGNI)

- chat / mood 仍然串行 (并行需要 ACP 请求多路复用或双进程, 留待后续)。
- 不动 TTS / LCD / 舵机执行链路。
- 不保留任何"离线本地模型"兜底: 断网时 mood 固定中性, chat 固定降级文案 (现状语义)。

## 验证

- `cargo fmt --all` / `cargo clippy --all-features --all-targets -- -D warnings` / `cargo test --workspace`
- 迁移的解析单测全绿; acp.rs 既有单测全绿
- Mac 本地实测 (zeroclaw homebrew, MiniMax-M2.5-highspeed, agent xiaobo):
  说话 → 回复 + 表情/动作正常; mood session 不出现在 chat 历史; 清空记忆不受影响
