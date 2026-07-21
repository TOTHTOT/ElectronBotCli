# audio-device-picker Spec Δ

> **基线**: `openspec/specs/audio-device-picker/spec.md` (v1.0.0, 2026-07-12 归档)
>
> **本 Δ**: 在 v1.0.0 之上, 增补 3 条 ADDED Requirements, 不修改任何现有 Requirement.
>
> 实现参照: `openspec/changes/2026-07-21-enhance-device-picker/{proposal.md, design.md, tasks.md}`.

## ADDED Requirements

### Requirement: 设备显示须呈现驱动字段以供辨识

picker overlay 渲染每个设备 SHALL 包含 driver 字段, 视觉上与设备名区分.

具体表现:

- `DeviceInfoDto.driver` 非空时, SHALL 用 dim 样式 (灰色 / 暗色) 渲染
- 设备名 SHALL 用主样式 (亮色) 渲染
- 通道数 / 采样率后缀 SHALL 用 dim 样式渲染
- 客户端 MUST NOT 仅依赖 `display` 字符串解析得到 driver (driver 必须以独立字段传输)

#### Scenario: 完整设备行渲染

- **WHEN** 列表中存在设备 `<driver="WASAPI", name="麦克风阵列", channels=2, sample_rate=48000>`
- **THEN** UI 行呈现 `WASAPI 麦克风阵列 (2ch / 48000Hz)`, driver 部分灰, name 部分亮, `(2ch / 48000Hz)` 灰

#### Scenario: driver 字段缺失

- **WHEN** 设备的 `driver` 字段为 `None`
- **THEN** UI SHALL 跳过 driver 部分, 仅显示 name + 通道 / 采样率

### Requirement: 服务端 VoiceManager 重建失败须在客户端提示

WHEN 服务端 `rebuild_voice` 因新设备不可用而返回 Err, 服务端 SHALL 广播 `ServerEvent::Error { message }`. 收到此 Error 且发生在最近一次设备切换提交 1 秒窗口内时, 客户端 SHALL 弹 transient overlay.

具体表现:

- overlay 标题: `设备切换失败`
- overlay 内容: `已保留 <旧设备名称>` + 错误明细
- overlay 自动 5 秒后关闭, **或** 用户按 Esc 立刻关闭
- 关闭后, 客户端 SHALL 清掉 `last_device_submit` 留痕, 避免与下一次 Error 串扰
- 失败 SHALL NOT 直接修改 `AppConfig.{speech_name, output_device}`, 用户保留旧设备

#### Scenario: rebuild 失败, 客户端弹 transient overlay

- **WHEN** 用户提交切换到 `<device A>`, 服务端 `rebuild_voice` 返回 Err
- **AND** 服务端广播 `ServerEvent::Error { message: "device A is busy" }` 距提交时间 ≤ 1 秒
- **THEN** 客户端 SHALL 弹 transient overlay `设备切换失败: 已保留 <旧设备名>`
- **AND** 配置字段仍为提交前的值

#### Scenario: 5 秒自动关闭

- **WHEN** transient overlay 弹出
- **THEN** 5 秒后 SHALL 自动关闭, **或** 用户 Esc 关闭 (哪个先发生用哪个)

#### Scenario: 不相关 Error 不触发 overlay

- **WHEN** 客户端收到 `ServerEvent::Error { .. }` 距最后一次设备切换提交 > 1 秒
- **OR** Error 与最近切换的 `kind` (输入 / 输出) 不匹配
- **THEN** 客户端 SHOULD 按现有 Error 通道处理, 不弹 transient overlay

### Requirement: picker overlay 内按 R 刷新列表须保留当前 cursor

WHEN 用户在 `Overlay::DevicePicker` 内按 `R`, 客户端 SHALL 重新发送 `ListInputDevices` 或 `ListOutputDevices`. overlay 内部状态切回 `loading = true`, 仅显示 `<加载中...>` 占位. 响应回来后:

- 若原 cursor 所指设备仍在新列表, cursor 保持
- 否则 cursor 退回到 idx 0 (`<系统默认>`)

#### Scenario: picker 内 R 刷新, 设备仍存在

- **WHEN** 用户在 picker 高亮 idx 2, 按 R
- **AND** 服务端响应包含原 idx 2 对应的设备
- **THEN** cursor 保持 idx 2, 显示完整列表

#### Scenario: picker 内 R 刷新, 设备不再存在

- **WHEN** 用户在 picker 高亮 idx 2, 按 R
- **AND** 服务端响应不包含原 idx 2 对应的设备
- **THEN** cursor 退到 idx 0 (`<系统默认>`)

#### Scenario: R 刷新期间屏蔽 Enter

- **WHEN** picker loading = true
- **THEN** Enter SHALL 被屏蔽, 仅 Esc 退出可用

## MODIFIED Requirements

(无 —— v1.0.0 的 6 条 Requirements 内容不需修改, 它们描述了原意, 本 change 既实装又新增 3 条增强)

## REMOVED Requirements

(无)
