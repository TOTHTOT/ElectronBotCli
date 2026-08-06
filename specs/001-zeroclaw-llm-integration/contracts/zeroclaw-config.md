# Contract: ZeroClaw 配置与部署

**Date**: 2026-08-02（2026-08-03 修订：配置改为用户自管理） | **Feature**: [spec.md](../spec.md)

## 二进制与文件布局（设备端 `~/ElectronBotCli/`）

```text
zeroclaw                     # 官方 aarch64-unknown-linux-musl 静态二进制（锁定版本，部署脚本只下发这个）
~/.zeroclaw/                 # zeroclaw 默认配置目录，完全由用户自己维护，本仓库不渲染、不下发
├── config.toml              # 用户自配：provider / api_key / model / agents
├── agents/<name>/workspace/
│   └── SOUL.md              # 用户自配人设（当前设备为 xiaobo）
└── data/                    # zeroclaw 自管理（sessions.db / brain.db），本仓库不直接读写
```

## 配置合约

- 本仓库**不渲染、不下发**任何 zeroclaw 配置；`ele_bot_server` spawn 时不传
  `--config-dir`，zeroclaw 走自身默认配置解析（`~/.zeroclaw`）
- provider / api_key / 人设（SOUL.md）缺失或错误时，zeroclaw prompt 报错，
  server 按降级路径播报"对话服务暂时不可用"——排查用 `zeroclaw doctor`
- 本仓库 AppConfig 的 `llm_api_base/api_key/model` 只喂 `analyze_mood`
  情感/动作分析链路，与 chat 无关

## 清空记忆命令链

TUI "清空对话记忆" → proto `Command::ClearLlmMemory`（新增可选变体，旧版本 client/server 交叉时忽略）→ server：
1. ACP `session/stop` 当前 session
2. `zeroclaw memory clear --yes`
3. `session/new` 重建 → 后续对话无历史、无记忆

## 健康检查与诊断（联调用）

- `zeroclaw status --format exit-code`
- `zeroclaw doctor`
- `zeroclaw memory stats`
