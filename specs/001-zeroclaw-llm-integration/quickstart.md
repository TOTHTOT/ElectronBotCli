# Quickstart: ZeroClaw 对话托管端到端验证

**Date**: 2026-08-02 | **Feature**: [spec.md](spec.md) | 合约: [zeroclaw-acp.md](contracts/zeroclaw-acp.md) / [zeroclaw-config.md](contracts/zeroclaw-config.md)

## 前置

- 设备：RK3566，`~/ElectronBotCli/` 已部署最新 `ele_bot_server` + `zeroclaw`（musl 静态版）
- 前置：用户已在设备上配好 zeroclaw（默认 `~/.zeroclaw`，含 provider/api_key/人设），设备联网
- 构建与部署：`RK_DEVICE=... RK_PASSWORD=... bash scripts/deploy_rk3566.sh ele_bot_server`（只下发二进制，不动 zeroclaw 配置）

## 场景 1：多轮上下文自动延续（P1 / SC-001 / SC-003）

1. 启动 `./ele_bot_server`，日志确认 zeroclaw 子进程 spawn 成功、ACP initialize + session/new 成功
2. 语音说："我叫小明"
3. 语音说："我叫什么名字"
4. **预期**：回答包含"小明"；日志中本仓库无任何历史拼接逻辑（SC-003：代码审查确认 session 历史方法已移除）

## 场景 2：长期记忆跨重启（P2 / SC-002）

1. 语音说："我喜欢听周杰伦"
2. `pkill ele_bot_server` 并重新启动（zeroclaw 数据目录不动）
3. 语音说："我喜欢听谁的歌"
4. **预期**：回答正确引用"周杰伦"；`zeroclaw memory stats` 总数 > 0

## 场景 3：清空记忆（FR-006）

1. TUI 执行"清空对话记忆"
2. 再问场景 2 的个人信息
3. **预期**：机器人不再记得；`memory stats` 归零；对话可继续（session 已自动重建）

## 场景 4：zeroclaw 不可用降级播报（P3 / SC-005）

1. `chmod -x zeroclaw`（或改名）后重启 `ele_bot_server`
2. 发起任意语音对话
3. **预期**：≤5 秒播报"服务不可用"类提示；主链路无 panic/卡死；恢复可执行权限后下次对话自动恢复

## 场景 5：延迟与资源测量（SC-004 + 调研遗留风险）

1. 日志时间戳对比：ASR 结束 → TTS 开始播报，与接入前基线对比，劣化 ≤20%
2. `ps -o rss= -C zeroclaw` 记录常驻 RSS（禁用 embeddings 状态），确认 2GB 设备上可接受
3. 顺带验证：`zeroclaw acp` 实际帧格式与 [zeroclaw-acp.md](contracts/zeroclaw-acp.md) 假设一致，不一致则修正合约与实现

## 通过标准

- 场景 1-4 全部符合预期；场景 5 指标达标
- `cargo fmt --all` / `cargo clippy --all-features --all-targets -- -D warnings` / `cargo check --all-features --all-targets` 全过（宪法 III）
- ACP 帧解析与 session 状态机单测 `cargo test -p ele_bot_server` 通过
