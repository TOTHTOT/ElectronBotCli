## Context

设置页 (`Route::Settings`) 当前只能改 4 项: 2 个 Wifi 文本字段 + 2 个设备选择器.
协议层 `AppConfig` 已经预留了 3 个 LLM 字段 (`llm_api_base` / `llm_api_key` / `llm_model`),
但 UI 没有入口 — 用户改 LLM 必须手动编辑 `config.toml`.

与此同时, 设置页文本输入 (`EditField`) 体验远低于可用线:
- `EditField { index, label, buffer }` 只有 buffer 字符串, 没有 cursor 概念
- 输入分发 (in `crates/ele_bot_client/src/input/mod.rs`) 只支持 `Char(c)` 末尾追加 + `Backspace` 末尾删除
- 不支持 Left / Right / Home / End / Delete / 定位插入
- 渲染 (`pages/settings.rs::render_setting_item`) 把整段编辑值反色, 不在文本里画 caret, 用户看不到插入点

这两个问题放在一起改是因为:
1. 改 `EditField` 必须改 input 分发; 一旦改分发, 自然引入 Left/Right, 顺带加 Delete / Home / End
2. 一旦 cursor 存在, 渲染光标就不再是可选优化, 而是必要
3. 一旦支持光标, 设置页所有文本字段 (现有 Wifi + 新加 LLM 三项) 一起受益, 不需要按字段类型差异化

这次仅改 `crates/ele_bot_client/**`, 不动 `ele_bot_proto` / `ele_bot_server`.

## Goals / Non-Goals

**Goals:**
- 在设置列表暴露 3 个 LLM 设置项 (api base / api key / model), 走现有的 `EditField` 文本编辑 + `SetConfig` 提交路径
- `EditField` 引入 char-level cursor, 支持完整编辑按键 (Left/Right/Home/End/Delete/Backspace/Char)
- 编辑态文本里有可见 caret, 用户清楚知道插入位置
- 操作提示条更新列出全部新按键
- 现有 wifi 字段编辑行为不回归 (只是 UX 升级)

**Non-Goals:**
- 不修改协议层 (`ele_bot_proto`), `AppConfig.llm_*` 字段已存在, 本次仅消费
- 不修改服务端 (服务端 `set_config` 已经接受 + 持久化 `AppConfig`, 不需要改)
- 不实现 `llm_api_key` 字段的可选掩码 (`****`) — mask 是 UX 安全优化, 留作未来 change
- 不实现登录态 / 设置项加密 / 设置项导入导出
- 不重做设置页布局 (不改布局, 不改导航结构)

## Decisions

### 决策 1: SETTINGS_LABELS 顺序 — LLM 插在 Wifi 后, 设备前

```
[0] Wifi 名称
[1] Wifi 密码
[2] LLM API 地址    ← 新增
[3] LLM API Key     ← 新增
[4] LLM 模型        ← 新增
[5] 麦克风           ← 原 2, 顺延
[6] 扬声器           ← 原 3, 顺延
```

**为什么这样排**: 设备项是 picker 走另一条路径, 跟文本编辑没关系, 隔离得越远越好. 把
LLM 三个文本字段挨着放, 视觉上和逻辑上都紧凑.

**考虑的别的方案**:
- 把 LLM 放在设备项之后 (idx 5/6/7): 让设备项继续在熟悉位置, 用户迁移成本低. 但 LLM 跟
  picker 共用一个分页, 视觉上分裂, 不太自然.
- 把 LLM 单独一页 (路由成 sub-page): 上线成本远超 UX 收益, 不值得. 当前一个屏幕 7 行 (含
  picker 弹窗) 是 ratatui 标准能装下的.

**风险**: 现有 wifi / picker 用户对 idx 顺序**有**依赖吗? 看了 `app/mod.rs:486-490` /
`510-512` 都是 match 分支, 没有裸 idx 假设; viewmodel 也按 `settings_items` 顺序迭代.
✅ 没有隐式假设.

### 决策 2: cursor 按 char 计数, 不按 byte offset

```rust
/// 用 char 索引避免 UTF-8 多字节断裂
pub struct EditField {
    pub index: usize,
    pub label: &'static str,
    pub buffer: String,
    pub cursor: usize,  // 0..=buffer.chars().count()
}
```

`insert_char` / `delete_back` / `delete_forward` 内部用 `buffer.char_indices()` 查 byte 边界,
外部永远只接触 char 索引.

**为什么不用 byte offset**: utf-8 字符 (中文 3 bytes, emoji 4 bytes) 会让 byte 边界不
等于 char 边界, 一旦忘了 char/byte 转换就会插入半个汉字 (Rust 的 `String::insert_str` 走
byte offset, 不能直接 `insert(2, "X")` — 如果 buffer 是 "你好", 索引 2 是 "好" 中间).

**考虑的别的方案**:
- 用 `unicode-segmentation` 库 (grapheme cluster): 完全 RFC 4648 正确. 但设置页只输入英文/
  中文/数字, 不需要 emoji 修饰符; 多一个依赖不划算.
- 用 byte offset + 内部 helper 防止越界: 调用方写错难以发现, 不如外部 API 直接走 char.

### 决策 3: caret 实现用块字符, 不用 `Frame::set_cursor_position`

```rust
// pages/settings.rs::render_setting_item 三段渲染
let before: String = buffer.chars().take(cursor).collect();
let after: String = buffer.chars().skip(cursor).collect();
let caret_span = Span::styled("\u{2588}", caret_style);  // 全角块字符 █
let line = Line::from_iter([
    Span::raw(indicator),
    Span::styled(format!(" {label}: ")),
    Span::styled(before, ed_style),     // 反色背景
    caret_span,                          // 反色背景的不同块
    Span::styled(after, ed_style),
]);
```

**为什么不用 `Frame::set_cursor_position`**: ratatui 提供 terminal-native caret 闪烁 /
硬件位置控制, 体验更好; 但 `crate::ui::mod.rs::render` 的弹窗 layer 在 EditField 文本
上方**覆盖绘制** (`frame.render_widget(Clear, popup_area)`), `set_cursor_position` 的
光标位置被 Clear 抹掉, 出现 "按键能看到光标移动但最终视觉位置不对" 的诡异 bug.

**折中方案**: 用一个 `█` / 空格占位列 + 反色背景, 视觉上是清晰的块 caret, 不依赖终端.
代价是 caret 不能闪烁 (终端闪烁需要 ANSI escape).

**风险**: CJK / emoji 双宽字符时, "光标块" 会显示 2 列宽, 而 `buffer.chars().take(cursor)`
按 char 计数 — 这会让视觉位置比预期偏右 1 列. 缓解: 块字符用**单宽**的 ASCII block
(`\u{2588}` Unicode 块字符在大部分终端是单宽, 但要试). 或者强制 caret 渲染前后插入
emoji-width 修正 — 暂不做, 用户报告再补.

### 决策 4: input 分发改用 `let-else` 风格, 不扩张 match

当前 `input/mod.rs:324-338` 的 EditField 分发是巨型 `match (code) { ... }`, 加按键
后会导致分支爆炸. 这次**不**重构 input 分发整体结构 (避免大范围变更提高 PR 风险),
但把按键处理抽成 `fn apply_key(field: EditField, code: KeyCode) -> EditField` 辅助:

```rust
fn apply_key(mut f: EditField, code: KeyCode) -> EditField {
    match code {
        KeyCode::Left => f.move_cursor_left(1),
        KeyCode::Right => f.move_cursor_right(1),
        KeyCode::Home => f.move_cursor_to_start(),
        KeyCode::End => f.move_cursor_to_end(),
        KeyCode::Backspace => f.delete_back(),
        KeyCode::Delete => f.delete_forward(),
        KeyCode::Char(c) => f.insert_char(c),
        _ => f,  // 未识别按键原样返回
    }
}
```

分发 match 里只关心 "Enter / Esc" 触发提交/取消, 其它所有按键都走 `apply_key`.

**为什么这样抽**: 输入分发本身行为不变, 只是把按键翻译成 `EditField` 操作的下沉到了
method. 后续如果加 Ctrl+A (全选) / Ctrl+K (kill to end) 都在 `EditField` 上加方法即可,
不动 input 分发.

## Risks / Trade-offs

| 风险 | 缓解 |
|---|---|
| Wifi 字段编辑行为回归 (之前只能末尾追加) | 现有 wifi 字段改成"末尾起始 cursor"后, 行为跟现状一致; 手动测 4 项确认 |
| CJK / emoji 输入时 caret 列偏移 | 用单宽 `█` 字符; 留待用户报, 再补 width 校正 |
| 服务端 LLM 接受 `llm_api_key == ""` 后调用失败 | 由 `llm-settings.spec` 兜底, 服务端不 panic, 失败由 LLM 服务商响应, 应用层处理 |
| `Frame::set_cursor_position` 在 popup 内被 Clear 抹掉 | 决策 3 决定不用它 |
| 索引扩展 4 → 7 项后, 别的模块隐式引用 idx 2/3 | search 旧代码确认没有 `cfg!(idx == 2)` / `cfg!(idx == 3)` (已确认 viewmodel 仅按 vec 迭代) |
| `delete_forward` 看似简单但 buffer 是空时不报错 | `delete_forward` 内部 `if cursor < chars.count() { remove }`, 空 buffer 自然 skip |

## Migration Plan

**部署**: 这是纯客户端代码改动, 不涉及服务端二进制 / 数据库 / 协议. 用户
拉新版本重新跑 `cargo run -p ele_bot_client` 即可生效. 配置文件 `config.toml` 是
向后兼容的 (3 个新字段走 `load_or_default` 的 toml 反序列化, 老文件没这三项会落入
`Default::default()` 的空字符串 / 默认模型).

**回滚**: 单 commit revert 即可, 没有迁移脚本要做.

**没有迁移的兼容性测试**: 因为 `AppConfig` 的字段是 `pub`, toml 反序列化天然容忍缺字段
— 旧的 4 字段 config.toml 不带 llm_* 也能加载, 用户进设置页后看到 3 项 <未配置> /
默认模型名, 行为正确.

## Open Questions

无. 设计范围内没有未决决策.
