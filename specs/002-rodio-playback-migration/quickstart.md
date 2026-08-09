# Quickstart: rodio 播放层迁移验证指南

本特性无外部接口变更 (proto 不变, `TtsPlayer` 公开 API 不变), 验证以
"两条播放路径在各平台实机表现"为准。contracts/ 目录因此为空 (N/A)。

## 前置条件

- `cargo build -p ele_bot_server` 通过 (macOS 本机)
- lckfb 设备在线 (`192.168.2.248`), 已部署 PCM2912A USB 声卡 + ElectronBot
- 三件套通过: `cargo fmt --all && cargo clippy --all-features --all-targets -- -D warnings && cargo check --all-features --all-targets`

## 场景 1: 非流式播放 (streaming: false) — P1

```bash
# 部署并重启设备端 server
RK_DEVICE=lckfb@192.168.2.248 RK_PASSWORD=lckfb bash scripts/deploy_rk3566.sh all ele_bot_server
ssh lckfb@192.168.2.248 'pkill -x ele_bot_server; sleep 1; cd ~/ElectronBotCli && (setsid nohup ./ele_bot_server </dev/null >/dev/null 2>&1 &)'
```

通过 WebSocket 发送 `TtsSpeak { streaming: false }`, 预期:

- 扬声器清晰播放完整语音 (无截断, 无"stream configuration is not
  supported" 错误)
- `server.log` 无播放失败 warn; 初始化阶段无声卡格式相关警告

## 场景 2: 流式播放 (streaming: true) — P1/P3

同上, 发送 `TtsSpeak { streaming: true }`, 预期:

- 边合成边出声, 完整播完不截断
- 日志出现 `TTS streaming playback done`, 无死等 (30s 内收尾)

## 场景 3: 不同采样率源 — P2

用单元测试或测试模式喂 24kHz/44.1kHz 的 `TtsAudio` 调用 `play()`, 预期
正常出声且语速音调正确 (mixer 自动重采样, 无需代码改动)。

## 场景 4: 代码结构验收 — P3/SC-003/SC-005

- `tts.rs` 播放层无 `SampleFormat` 分支、无设备型号特判
- 行数对比: `git diff --stat b8db23d` 播放层净减少 ≥50%
- `grep -r OwnedOutputStream crates/` 无残留引用

## 场景 5: 回归 — 语音全链路

在 lckfb 上对机器人说一句话, ASR → zeroclaw → TTS 全链路正常回复出声。

## 预期外处理

任一实机场景失败 → 回到 spec 重新评估 (见 checklists/requirements.md
Notes), 不削需求。
