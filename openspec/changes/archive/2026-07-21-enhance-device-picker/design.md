# Design

## 背景

`audio-device-picker` 已在 main (spec 落, 实装未落). `voice-realtime` 把 `VoiceManager` 热重建也带了进来. 本 change 收口.

主分支已具备:

- `voice::list_input_devices()` / `list_output_devices()` —— 服务端枚举
- `voice::find_input_device(name)` / `find_output_device(name)` —— 服务端按名查找
- `SharedState::rebuild_voice()` —— 热重建 (50ms 软替换, 旧实例 `running` 标志置 false 后 sleep)
- `SharedState::set_config` 检测 `speech_name` / `output_device` 字段变化自动触发 `rebuild_voice`
- `AppConfig { speech_name, output_device, ... }` —— 客户端已序列化保存

本 change 补:

- 协议层枚举消息链路
- 客户端 picker UI + Route/Overlay 子状态
- 3 个新 spec requirement: driver 字段渲染, rebuild 失败 transient overlay, picker 内 R 刷新

## 关键决策

### D1: driver 独立字段 vs 仅 display 字符串

服务端 `voice::DeviceInfo::new(name, channels, sample_rate, idx, driver)` 已经在 `display` 里拼成 `"{driver} {name} ({channels}ch / {sample_rate}Hz)"`. 备选:

- (a) 只发 `display`, 客户端解析字符串得到 driver
- (b) `display` + `driver` 单独两个字段

选 **(b)**:

- 不在 wire 上做 "可解析 display 字符串" 约定. 改 display 模板就破解析.
- 客户端布局自由: driver 着色, name 亮色, channels/sample_rate dim. 容易重排.
- 抓包 / 日志可读, 不会出现"为啥 display 又变了"
- 没有老客户端 (没 device picker 入口)

### D2: picker 用 Overlay 新变体, 不引入 Route 新子状态

- (a) `Overlay::DevicePicker(SelectingField, Vec<DeviceInfoDto>)` —— 与 `Overlay::EditField` 平行
- (b) `Route::Settings { ..., selecting: Option<SelectingField> }` —— 多加一个 Option 字段

选 **(a)**:

- 现有架构是 `Overlay` enum 复合, `handle_overlay` 一处集中处理 modal 行为. 加一个变体 = 加一个 match 分支, 路径清晰.
- `Route::Settings` 已经塞了 `editing: Option<EditField>`, 再塞 `selecting` 让 Route 状态空间 4x. 维护难.
- transient overlay (失败提示) 复用同一机制: 也进 `Overlay::*`. 一致.

(b) 不算坏, 但要走 `handle_by_mode` 加分支、`handle_settings` 内根据 selecting 选分支、入 Settings 默认 selecting=null, 复杂度更高.

### D3: rebuild 失败 transient UX

服务端 `rebuild_voice` 失败时, 已广播 `ServerEvent::Error { message }`. 备选:

- (a) 客户端不区分, 跟其他 Error 同一通道 (现有行为)
- (b) 用 `last_device_submit: Option<Instant>` 留痕, 仅 1 秒窗口内的 Error 视为"刚才切设备失败", 弹 transient overlay

选 **(b)**:

- `ServerEvent::Error` 是公共通道, 其他场景 (USB 断开, 配置反序列化失败) 也会用. 仅"刚刚切设备失败"是用户预期内的反馈.
- 收 Error 时检查 `Instant::now() - last_device_submit <= 1s && last_device_submit_kind 一致`, 才弹 overlay. 避免串扰.
- `Overlay::DeviceSwitchFailure { kind, old_device_name }` 自动 5 秒关或 Esc 关. 关时清掉 last_device_submit.

`last_device_submit_kind` 区分输入还是输出, 防止切麦克风失败时弹出关于扬声器的 overlay.

### D4: picker 内 R 刷新, 不退到列表模式

- (a) picker 内部切换 `loading = true`, 收到响应后保留 cursor (设备还在) / 退回 idx 0 (设备不在)
- (b) picker 关闭, 跳回列表, 重新进

选 **(a)**:

- 用户上下文保留 (切设备时按 R 找新耳机, 不希望弹回列表再点一次)
- 与 spec 第 4 条 "当前选择项 SHALL 保留" 精神一致
- 实现简单: `SelectingField { loading: bool }` + overlay 渲染分支

(b) 多绕一段, 用户体验下降, 无人获益.

## 取舍

### T1: 持久化时机

picker 提交后只 `SetConfig` + 设 `last_device_submit*`, **不写盘**.

- "实时切换" 把服务端重建时间压在选中后 50ms 内, 关键路径不要 IO
- 写盘 (TOML) 是 desktop 端的标准操作, 但在本 change 不创建新的 save 入口. 维持现状 (用户后续 choice)
- 注: 当前代码里没有显式 save 入口, 这一点本身就是 gap. 不在本 change 修, 标到 future work

### T2: 设备列表拉取频率

只在三种时机拉:

1. 进入 `Route::Settings` (Nav → Settings 按 Enter)
2. `SettingsEvent::RefreshList` 列表模式按 R
3. picker 内按 R (E2 = a)

不主动监听系统设备变化. 原因:

- cpal 无标准跨平台热插拔 API
- 自实现需平台特定 code (Windows IMMDeviceEnumerator 通知 / Linux udev / macOS CoreAudio kAudioHardwarePropertyDevices). 复杂度高
- 用户明确不要热插拔自动刷新

### T3: driver 不是 enum, 是字符串

`device_info.driver: Option<String>`, 不是 enum.

- cpal 在不同 OS 的 driver 名字五花八门: WASAPI / DSound / MME (Windows), ALSA / PulseAudio / JACK (Linux), CoreAudio (macOS)
- enum 化会让客户端永远滞后于新驱动出现
- 比较 / 排序 / 过滤交给客户端 (客户端可把已知 driver 名 map 到 color chip)

### T4: failure window 多长

`last_device_submit` 1 秒窗口.

- 服务端 `rebuild_voice` 含 `std::thread::sleep(60ms)`, 加上 ASR 线程退出 + cpal stream 重启, 端到端通常在 100~300ms 内完成或广播 Error
- 1 秒够覆盖正常延迟, 又短到不会跟其他 Error 串扰
- 长窗口会让"5 分钟前提交过设备, 现在 USB 断了"也被识别成切设备失败, 反直觉

## 数据流

**正常路径**:

```
用户在 Settings 列表, 高亮 "麦克风" 行, 按 Enter
        ↓
handle_settings: KeyCode::Enter, selecting.is_none() && editing.is_none()
        ↓
SettingsEvent::EnterPicker
        ↓
settings::handle: Route::Settings.selecting = Some(SelectingField { kind: Input, cursor: 0, loading: devices.inputs.is_empty() })
                  Overlay::DevicePicker(selecting, devices.inputs.clone())
                  若 loading=true: 顺便发 ClientMessage::ListInputDevices (因为还没拉到列表)
        ↓
UI 重绘: 中心弹窗, 单列, 高亮 idx 0
        ↓
用户 ↑ → SettingsEvent::PickerUp → cursor = (cursor - 1 + len) % len
用户 ↓ → SettingsEvent::PickerDown → cursor = (cursor + 1) % len
用户 Enter → SettingsEvent::PickerConfirm
        ↓
app.config.speech_name = picker.devices[cursor-1].name.clone() (idx 0 → "")
last_device_submit = Some(Instant::now())
last_device_submit_kind = Some(Input)
route.selecting = None
overlay = None
tx.send(ClientMessage::SetConfig { config: app.config.clone() })
        ↓
服务端 ws.rs::handle_client_msg → set_config
        ↓
state.set_config 检测 speech_name != old_mic → rebuild_voice
        ↓
成功: 替换 self.voice, 旧 Drop, 无 event 需广播
失败: out_tx.send(ServerEvent::Error { message })
```

**失败路径**:

```
客户端 ws 收到 ServerEvent::Error { message }
        ↓
if last_device_submit.is_some()
   && now - last_device_submit <= 1s
   && last_device_submit_kind 一致
   && 此次调用 set_config:
        ↓
overlay = Some(Overlay::DeviceSwitchFailure { kind, old_device_name: app.config.speech_name.clone() })
        ↓
5 秒后, 或用户 Esc → overlay = None; last_device_submit = None; last_device_submit_kind = None
        ↓
恢复 Settings 列表, 行内显示的仍为旧 device (config 未实际切换)
```

**进入 Settings 自动拉列表**:

```
Nav → Settings 按 Enter
        ↓
Route::from(MenuItem::Settings) → Route::Settings { selected: 0, editing: None, selecting: None }
        ↓
入口逻辑 (在从 Nav 进入的代码点, 或 From<MenuItem> for Route 后追加一次性副作用):
tx.send(ListInputDevices); tx.send(ListOutputDevices);
        ↓
服务端响应 → InputDevices / OutputDevices 事件
        ↓
app.devices.{inputs, outputs} 更新; 若 selecting.loading → 清 loading
```

## 验收

(与 proposal.md 一致)

- TUI 进设置 → 自动收两条事件, 渲染 `<系统默认>` + 设备
- picker ↑↓ Enter Esc R 全可用
- 切换成功后服务端真实换设备, 无重启
- 切换失败 transient overlay 弹 5 秒或 Esc
- driver dim gray, name 亮色, 通道/采样率 dim

## Future work (不在本 change)

- 设备热插拔主动监听 (平台特定)
- 写盘入口 (save 按钮 / 退出时自动 save)
- 视频设备选择扩展
