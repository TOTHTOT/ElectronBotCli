# Quickstart: 设置页音量调节与测试输入框优化

端到端验证指南。前置：已完成 tasks.md 全部任务，本地三件套通过。

## 场景 1: 单元/协议测试（无硬件）

```bash
cargo test -p ele_bot_proto    # AppConfig 新旧字段兼容
cargo test -p ele_bot_server   # 采集增益乘法/clamp
cargo test -p ele_bot_client   # TextInput 编辑、设置页音量行
```

预期：全绿。

## 场景 2: 扬声器音量（实机 lckfb）

前置：部署 `RK_DEVICE=lckfb@192.168.2.248 RK_PASSWORD=lckfb bash scripts/deploy_rk3566.sh all ele_bot_server`，client 已连接。

1. client 进入设置页，选中「扬声器音量」行
2. 按 `←` 若干次到 ~30%，切到 TTS 测试页输入一句话回车
3. **预期**: 机器人播放声音明显变小；设置页显示值与 server 持久化一致
4. 按 `→` 回到 100%，再播一次
5. **预期**: 音量恢复
6. 重启 client 与 server（`ssh lckfb@192.168.2.248 'pkill -x ele_bot_server; cd ~/ElectronBotCli && (setsid nohup ./ele_bot_server </dev/null >server.log 2>&1 &)'`），重开设置页
7. **预期**: 音量值保持重启前的值（config.toml 持久化）

## 场景 3: 麦克风增益 + 实时电平（实机）

1. 设置页选中「麦克风音量」行，对着机器人说话
2. **预期**: 电平指示实时跳动（复用现有 Volume 广播）
3. 按 `←` 降到 ~20%，继续说话
4. **预期**: 电平指示明显变低（增益后信号）；语音唤醒/识别距离变近
5. 按 `→` 恢复，电平回升

## 场景 4: 测试输入框（TTS/LLM 页）

1. TTS 测试页输入「你好世界」，光标左移两次，输入「，」
2. **预期**: 文本为「你好，世界」，caret 在「，」后
3. 按 `Backspace` 一次
4. **预期**: 删除整个「，」（无乱码、无半个字符）
5. 输入超长文本（>输入框宽度）继续打字
6. **预期**: 视图横向滚动，caret 恒可见
7. 按 `Ctrl+U` 清空，重输一句话回车
8. **预期**: 正常提交播放；`↑`/`↓` 调速、`M` 切流式仍可用
9. LLM 测试页重复 1-5 + 验证 `F2` 清空记忆弹窗仍正常

## 场景 5: 协议兼容（降级路径）

1. 用本特性前的旧 client 二进制连接新 server
2. **预期**: 连接正常、设置/测试功能正常；旧 client 保存一次配置后音量为
   100%（已知限制，不报错即通过）
