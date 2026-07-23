# settings-text-input Specification

## Purpose

设置页文本输入 (`EditField`) 提供字符级光标 + 完整按键支持 + 可见 caret 渲染,
适用于所有文本字段 (当前: Wifi 名称 / Wifi 密码 / LLM API 地址 / LLM API Key /
LLM 模型). 字符索引按 `char` 而非 byte 计数, UTF-8 安全. 输入分发支持
Left/Right/Home/End/Delete/Backspace/Char + Enter/Esc, 其它按键原样穿透.
## Requirements
### Requirement: EditField 持有字符级 cursor

`EditField` 结构 SHALL 新增字段 `cursor: usize`, 表示"光标在 buffer 中的字符索引".
索引 SHALL 按 Rust `char` 计数 (`0..=buffer.chars().count()`), **不**按 byte offset —
保证 UTF-8 多字节字符 (中文 / emoji) 视为单个编辑单位, 不会出现半个字.

`EditField::new` SHALL 接收 `cursor` 参数 (而不是默认 0), 调用方负责传入合法值.
当 buffer 已有内容时, 推荐把 cursor 初始化在 `buffer.chars().count()` (末尾), 方便
追加; 也可以传 0, 用于 "覆盖式" 输入.

`EditField` SHALL 提供 char-level 工具方法:
- `insert_char(&mut self, c: char)`: 在 cursor 处插入字符, cursor+=1; 隐式上界 clamp 到 `chars().count()+1`
- `delete_back(&mut self)`: 删 cursor 前一个字符 (cursor > 0 时有效), cursor-=1
- `delete_forward(&mut self)`: 删 cursor 位置字符 (cursor < chars().count() 时有效), 长度不变
- `move_cursor_left(&mut self, n: usize)`: cursor 减 n, 下界 clamp 到 0
- `move_cursor_right(&mut self, n: usize)`: cursor 加 n, 上界 clamp 到 `chars().count()`
- `move_cursor_to_start(&mut self)` / `move_cursor_to_end(&mut self)`: cursor 置 0 / 末尾

所有方法 SHOULD 在内部用 `buffer.char_indices()` 转换 char 索引 ↔ byte 索引, 不让
上层直接接触 byte offset.

#### Scenario: 编辑中文 buffer 不破坏字符
- **WHEN** `EditField.buffer = "你好"`, `cursor = 1` (在 "好" 之前)
- **THEN** `delete_back()` SHALL 删掉 "好", `cursor = 0`, `buffer = "你"`
- **AND** `buffer.len()` SHALL 从 6 bytes 变为 3 bytes (中间不出现非法 UTF-8)

#### Scenario: cursor 越界 clamp
- **WHEN** `buffer = "abc"`, `cursor = 1`
- **THEN** `move_cursor_left(5)` SHALL 把 cursor clamp 到 `0`
- **AND** `move_cursor_right(99)` SHALL 把 cursor clamp 到 `3` (末尾位置)
- **AND** `insert_char('X')` SHALL 把 buffer 改为 `"Xabc"`, `cursor = 1`

### Requirement: 设置页输入分发支持完整编辑按键

`handle_by_mode` 在 `Route::Settings { editing: Some(_), .. }` 状态下 SHALL 支持以下按键:

| 按键 | 行为 |
|---|---|
| `KeyCode::Left` | cursor 左移 1 |
| `KeyCode::Right` | cursor 右移 1 |
| `KeyCode::Home` | cursor 置 0 |
| `KeyCode::End` | cursor 置 `buffer.chars().count()` |
| `KeyCode::Delete` | 调 `delete_forward()`, 长度不变 |
| `KeyCode::Backspace` | 调 `delete_back()`, cursor-=1 |
| `KeyCode::Char(c)` (非控制键) | 调 `insert_char(c)` 在 cursor 处 |
| `KeyCode::Enter` | 现有: 提交编辑 |
| `KeyCode::Esc` | 现有: 取消编辑, 撤销 buffer |

未列出的按键 (`Tab`, `PageUp`, `PageDown`, 功能键) SHALL NOT 修改 buffer 也不移动 cursor.

`Backspace` 与 `Delete` SHALL NOT 在切换 wifi / llm 等不同字段之间产生行为差异.

#### Scenario: 用户在 buffer 中间插入字符
- **WHEN** `buffer = "abc"`, `cursor = 1`, 用户按 `Char('X')`
- **THEN** `buffer` SHALL 变为 `"aXbc"`, `cursor` SHALL 等于 `2`

#### Scenario: Home/End 跳转
- **WHEN** 用户在 `buffer = "abcdef"` 中按 `Home`
- **THEN** `cursor` SHALL 等于 `0`
- **AND** 再按 `End`, `cursor` SHALL 等于 `6` (末尾, char count)

#### Scenario: 移动光标后删除
- **WHEN** `buffer = "abcdef"`, `cursor = 3`, 用户按 `Delete`
- **THEN** `buffer` SHALL 变为 `"abcef"`, `cursor` 仍等于 `3`
- **WHEN** 继续按 `Backspace`
- **THEN** `buffer` SHALL 变为 `"abcf"`, `cursor` SHALL 等于 `2`

#### Scenario: 未识别按键不修改 buffer
- **WHEN** 用户在编辑态按 `KeyCode::Tab`
- **THEN** `buffer` 和 `cursor` SHALL **不**变

### Requirement: 编辑态渲染可见块状 caret

设置页 `render_setting_item` 在编辑态 SHALL 把 buffer 拆成 3 段渲染:
1. `buffer.chars().take(cursor)` — 反色背景高亮, 与现有编辑态视觉一致
2. caret 位置 — 用一个**反色块字符**占位, 视觉上明显区别于普通文本
3. `buffer.chars().skip(cursor)` — 继续反色背景高亮

caret 字符 SHALL 在所有终端上至少 1 列宽, 不能因为 emoji width (2 cols) / CJK width (2 cols)
跨过多个字符位置; 推荐用 ASCII 范围 block 字符 (e.g. `█`, 全角块 `▌`) 或者一个空格 + 反色 + 背景.

caret SHALL 跟 cursor 一起移动: 用户按 `Left` 后下次渲染 SHOULD 在新位置看到 caret.

`render` SHALL NOT 使用 `Frame::set_cursor_position` (与 overlay 内联渲染冲突, 见已知
约束). 块字符模式渲染是唯一允许的 caret 实现.

#### Scenario: 编辑空字符串 (cursor 在 0)
- **WHEN** buffer 是 `""`, cursor 是 `0`
- **THEN** 渲染 SHALL 仅显示 caret 块字符 (反色背景), 没有普通文本段
- **AND** 不出现 emoji width / CJK width 引发的列偏移

#### Scenario: caret 随光标移动
- **WHEN** 渲染前光标在 `cursor = 3`, buffer 是 `"abcdef"` ("abc|def")
- **THEN** 渲染 SHALL 显示为 `"abc"` + caret + `"def"`, caret 列位置 SHALL 在 "c" 之后
- **AND** 当用户按 `Left` 后下一帧, caret 位置 SHALL 移到 "c" 和 "b" 之间

### Requirement: 操作提示条更新按键列表

设置页信息条文本 SHALL 改为同时列出全部编辑态按键:
`"[Enter] 保存  [Esc] 取消  [Backspace] 删前  [Delete] 删后  [←→] 移动  [Home/End] 跳首尾"`

非编辑态信息条文案 SHALL 保持不变.

#### Scenario: 进入编辑后提示更新
- **WHEN** `in_edit_mode == true`
- **THEN** 渲染 SHALL 显示包含 `[←→]` 与 `[Home/End]` 提示的新字符串
- **AND** 该字符串 SHALL 在 buffer == "" 时仍能完整显示在 info bar 3 行高内 (纯文本不超过 80 列, 终端宽 < 80 时截断)
