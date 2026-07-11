# ElectronBotCli — Claude 协作约定

## 项目简介

ElectronBot 的 Rust 命令行上位机, 由三个 crate 组成的 workspace:

| crate | 职责 |
|---|---|
| `crates/ele_bot_proto` | 客户端/服务端共享的协议类型 (`ClientMessage` / `ServerEvent`) |
| `crates/ele_bot_server` | 服务端: 串口舵机控制、摄像头、ASR/TTS、LLM |
| `crates/ele_bot_client` | 客户端: ratatui TUI, 通过 WebSocket 与服务端通信 |

TUI 的输入分两层, 修改输入相关代码前必读:
- `crates/ele_bot_client/src/input/mod.rs` 的 [`handle_event`] / [`handle_by_mode`]
- 派发契约写在 `handle_event` 的 rustdoc 里, **不要绕过它直接调 `App` 方法**

## 注释规范

### 1. 公共 API 必须有 rustdoc

任何 `pub` 函数、结构体、枚举、trait, 必须有 `///` 开头的文档注释. 至少包含:

- **一句话职责** —— 它干什么, 不干什么
- **边界 / 不变量** —— 调用方需要知道的前提 ("此函数只翻译按键, 不调 App 方法")
- **`# Examples`** (推荐) —— 至少一个真实用法, 用 ` ```rust,ignore ` (避免 doctest 拉起整个运行时)

参考样例: `crates/ele_bot_client/src/input/mod.rs` 的 `handle_event`.

### 2. 模块头注释

每个 `.rs` 文件顶部用 `//!` 写 2-4 行说明本模块职责, 以及与其它模块的关系. 例:

```rust
//! 设置事件
//!
//! SettingsEvent 只覆盖 Settings 列表页的按键语义.
//! EditField overlay 是另一种输入模式, 由 handle_overlay 直接处理,
//! 不走 SettingsEvent.
```

### 3. 行内注释用中文

代码内的 `//` 行内注释使用中文, 解释**为什么**而不是**做什么** (做什么看代码就知道). 例:

```rust
// 设备状态页: Enter = 连接/断开, 走 MenuEvent::ConnectDevice
// (而不是 Route::from(DeviceStatus) —— 那是死循环)
```

不要写显而易见的注释 (`// i += 1` 这种).

### 4. 日志 / 用户可见字符串

`log::*!` 用中文短句; UI 字符串遵守现有约定 (中文为主, 个别英文术语保留).

## 提交流程 (强制)

提交前**必须**依次通过以下检查, **任何一项失败都不提交**:

```bash
# 1. 格式化
cargo fmt --all

# 2. clippy (CI 用 -D warnings, 本地同)
cargo clippy --all-features --all-targets -- -D warnings

# 3. 编译检查
cargo check --all-features --all-targets
```

完整命令可一次性跑:

```bash
cargo fmt --all && \
  cargo clippy --all-features --all-targets -- -D warnings && \
  cargo check --all-features --all-targets
```

> Windows PowerShell 下用 `;` 或单独跑三次. CI (`.github/workflows/ci.yml`) 用的是这三项的子集 (fmt / clippy / build + test), 本地先跑这三项能避免大多数 PR 失败.

## 其它约定

### Commit 信息

- **首行格式**: `[<类别>/] <中文短句>`, 类别用中文关键词, 常用集合: `新增` `修复` `发布` `优化` (允许按需扩展, 例 `重构/` `回滚/` `文档/`).
- 中文短句, 描述**为什么**而非**做了什么**.
- 首行不超过 50 字, 不加 emoji.
- **正文追加报错日志**: 若本次提交涉及修复报错或回归, 在主信息后**空一行**, 追加相关报错日志原文 (包括堆栈/调用栈片段), 不要截断关键行, 不要二次转述. 例:

  ```
  修复/启动时 TermLogger 静默写入失败丢日志

  07:57:37 [ERROR] TermLogger init failed: handle is not a terminal
  Caused by: Windows stderr detached in Tauri child process
  ```

- **不**沿用英文 `feat:` / `fix:` 前缀, 改用上面的中文 `[<类别>/]`.
- 例: `新增/服务端日志同时输出到终端和文件`

### 修改前先读

任何文件, 先用 `Read` 看完整内容再动. 不要基于猜测改.

### 任务颗粒度

超过 3 步的改动, 用 `TaskCreate` 列任务. 每完成一项 `TaskUpdate` 标记. 这样能让你看清进度, 也方便我中断后接着干.

### 回答语言

始终使用中文. 代码注释、commit 信息、对话回复一律中文. 技术术语可保留英文 (如 `AppEvent` / `Route` / `dispatch`).