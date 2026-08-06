# Contract: ZeroClaw ACP 接口（JSON-RPC 2.0 over stdio）

**Date**: 2026-08-02 | **Feature**: [spec.md](../spec.md)

本仓库 `ele_bot_server` 作为 ACP client，spawn `zeroclaw acp` 子进程，stdin/stdout 走 JSON-RPC 2.0。

**帧格式已实测确认（zeroclaw v0.8.3）**：NDJSON，每行一个完整 JSON 对象，无 Content-Length 头。
回复分片通知形状：`{"method":"session/update","params":{"update":{"sessionUpdate":"agent_message_chunk","content":{"text":"..."}}}}`；
`session/new` 响应含 `sessionId` 与 `workspaceDir`；provider 故障返回 `error.code = -32603`（message 含底层原因，如 403 AccountOverdueError）。
doubao ark 端点连通性已实测（请求能到达 ark 并返回业务错误，配置解析正常）。

## 使用的方法子集

### initialize

```json
{ "jsonrpc": "2.0", "id": 1, "method": "initialize",
  "params": { "protocolVersion": 1, "clientCapabilities": {} } }
```

- 成功：返回 server capabilities → client 进入 Initialized
- 失败/超时（3s）：判定 zeroclaw 不可用

### session/new

```json
{ "jsonrpc": "2.0", "id": 2, "method": "session/new",
  "params": { "cwd": "<session 工作目录, 仅作 cwd; 配置/人设由 zeroclaw 自身加载>", "mcpServers": [] } }
```

- 成功：返回 `sessionId` → 进入 Ready
- 多轮对话复用同一 sessionId；清空记忆时 stop 后重建

### session/prompt

```json
{ "jsonrpc": "2.0", "id": 3, "method": "session/prompt",
  "params": { "sessionId": "<id>",
    "prompt": [ { "type": "text", "text": "<用户文本>" } ] } }
```

- 回复以 notification 流式推送（`session/update`，text chunk），client 聚合直到收到 prompt 的 final response
- **超时：5s**（spec US3）；超时/错误 → `Err` → 语音链路播报"服务不可用"
- 本仓库只取最终文本，不消费工具调用/权限类 notification

### session/stop（清空记忆 / 优雅退出时）

```json
{ "jsonrpc": "2.0", "id": 4, "method": "session/stop",
  "params": { "sessionId": "<id>" } }
```

## 错误与降级约定

| 场景 | client 行为 |
|------|-------------|
| 子进程未启动/启动失败 | chat 直接 Err，播报不可用 |
| 请求超时 | cancel 该请求，Err，播报不可用 |
| 子进程退出（ Broken ） | 标记 Broken；下次 chat 自动重 spawn + initialize + session/new 恢复 |
| JSON-RPC error response | Err，日志记录原文，播报不可用 |

## 版本锁定

- ACP 为外部标准协议（agentclientprotocol.com），本合约只依赖上列 4 个方法
- zeroclaw 版本随 `assets/zeroclaw/` 二进制锁定；升级 zeroclaw 前必须重跑 quickstart 全部场景
