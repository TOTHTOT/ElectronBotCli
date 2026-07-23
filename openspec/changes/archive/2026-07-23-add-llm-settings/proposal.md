## Why

设置页当前只能改 Wifi 名称 / Wifi 密码 / 麦克风 / 扬声器 4 项. 协议层 `AppConfig` 已经预留了
`llm_api_base` / `llm_api_key` / `llm_model` 3 个字段, 但 UI 完全不暴露 — 用户要换 LLM 厂商或
模型只能手动编辑 `config.toml`. 同时 `EditField` 输入路径体验差: 没有 cursor 位置, 不能
Left/Right/Home/End/Delete, 渲染时整段反色覆盖, 用户看不到插入点. 这次同时修这两件事.

## What Changes

- **新增 3 项设置**: `LLM API 地址`, `LLM API Key`, `LLM 模型`. 在设置列表里插在麦克风/扬声器
  之前, 占原 idx 2/3/4, 设备项顺延到 5/6. 按 Enter 进入文本编辑, 走现有 `EditField` 路径.
  提交后通过 `ClientMessage::SetConfig` 把新 `AppConfig` 发给服务端, `config.toml` 持久化.
- **`EditField` 加 cursor**: 数据结构新增 `cursor: usize` (字符索引, `0..=buffer.chars().count()`).
  按字符处理 (非 byte offset), UTF-8 安全.
- **输入按键扩展**: `Left / Right / Home / End / Delete` 新加进来; `Backspace / Char(c)` 改成
  基于 cursor 的 splice 语义.
- **块状 caret 渲染**: 编辑态把 buffer 拆 `[..cursor] + █ + [cursor..]` 三段渲染, 光标位置
  用反色块字符显示, 跟现有反色高亮兼容. 不用 `Frame::set_cursor` (跟 overlay 内联渲染冲突).
- **操作提示条更新**: 列出全部支持按键 (`←→` 移动 / `Home/End` 跳首尾 / `Delete` 删后 等).

### Capabilities

### New Capabilities

- `llm-settings`: 设置页暴露 LLM 三个配置项 (api_base / api_key / model) 并走 SetConfig 提交.
- `settings-text-input`: `EditField` 字符级光标 + 完整按键 + caret 渲染, 适用于设置页所有
  文本字段 (现有 wifi 也享受, 但不改 wifi 提交逻辑).

### Modified Capabilities

无. `audio-device-picker` / `voice-realtime` 不涉及 spec 级需求变更.

## Impact

- 改动范围: `crates/ele_bot_client/**` (协议层 `ele_bot_proto` 和服务端 `ele_bot_server` **不动**).
- 文件清单:
  - `crates/ele_bot_client/src/app/mod.rs` — `SETTINGS_LABELS` 加 3 项 + 索引常量 + `begin/commit_settings_edit` 分支
  - `crates/ele_bot_client/src/app/route.rs` — `EditField` 加 `cursor` 字段 + char-level 工具方法
  - `crates/ele_bot_client/src/input/mod.rs` — overlay 分发加 Left/Right/Home/End/Delete; Char/Backspace 改用 cursor
  - `crates/ele_bot_client/src/ui/viewmodel/settings.rs` — `SettingsViewModel` 镜像 cursor + items 加 3 项
  - `crates/ele_bot_client/src/ui/pages/settings.rs` — `render_setting_item` 三段渲染 + 信息条文案
- 风险: 现有 wifi 编辑也走新光标路径, 行为更鲁棒 — 已测试无回归.
- 不引入新依赖, 不动 cargo.toml.
