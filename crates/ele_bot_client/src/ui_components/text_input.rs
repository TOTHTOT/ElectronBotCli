//! 内置单行文本输入组件 [`TextInput`]
//!
//! TTS / LLM 测试页共用的输入框状态与渲染辅助。抽成独立组件的原因:
//! 原实现直接往 `String` 上 `push` / `pop`, 不支持光标移动与前向删除,
//! 也不能处理多字节字符 (中文) 的按字删除。
//!
//! ## 不变量
//!
//! - `cursor` 是**字符** (char) 索引, 不是 byte 索引, 范围
//!   `0..=buffer.chars().count()`
//! - `buffer.chars().take(cursor)` 是 caret 之前的内容,
//!   `buffer.chars().skip(cursor)` 是 caret 之后
//! - 工具方法内部用 saturating 算术 + `chars().count()` 兜底,
//!   不让 cursor 漂出合法范围
//!
//! 与 `crate::app::route::EditField` 的关系: 编辑逻辑同源 (char 级光标),
//! 但 `EditField` 绑定 Settings overlay 的 label/index, 本组件不带这些
//! 字段, 供测试页长期持有。

use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

/// 单行文本输入框状态 (buffer + char 级光标)
#[derive(Debug, Clone, Default)]
pub struct TextInput {
    buffer: String,
    /// 光标在 buffer 中的**字符**索引 (`0..=buffer.chars().count()`)
    cursor: usize,
}

impl TextInput {
    /// 新建空输入框
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 当前文本
    #[must_use]
    pub fn text(&self) -> &str {
        &self.buffer
    }

    /// 光标位置 (char 索引)
    #[must_use]
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// 字符总数
    fn char_count(&self) -> usize {
        self.buffer.chars().count()
    }

    /// char 索引转 byte 索引: 第 `char_idx` 个 char 的起始字节,
    /// 越界时返回 `buffer.len()` (即末尾)
    fn byte_index(&self, char_idx: usize) -> usize {
        self.buffer
            .char_indices()
            .nth(char_idx)
            .map_or(self.buffer.len(), |(b, _)| b)
    }

    /// 在 cursor 处插入 `c`, cursor += 1. 光标在末尾时等价于 push.
    pub fn insert_char(&mut self, c: char) {
        let cursor = self.cursor.min(self.char_count());
        let byte_idx = self.byte_index(cursor);
        self.buffer.insert(byte_idx, c);
        self.cursor = cursor + 1;
    }

    /// 删 cursor 前一个字符 (cursor > 0 时有效). 返回是否发生了删除.
    pub fn delete_back(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        let prev = self.cursor - 1;
        let start = self.byte_index(prev);
        let end = self.byte_index(self.cursor);
        self.buffer.drain(start..end);
        self.cursor = prev;
        true
    }

    /// 删 cursor 位置字符 (cursor < `chars().count()` 时有效). 返回是否删除.
    ///
    /// 与 `delete_back` 区别: Backspace 删前面的字符, Delete 删后面的字符;
    /// 删除后 `cursor` 不变 (删的是它"指着"的那个字符).
    pub fn delete_forward(&mut self) -> bool {
        if self.cursor >= self.char_count() {
            return false;
        }
        let start = self.byte_index(self.cursor);
        let end = self.byte_index(self.cursor + 1);
        self.buffer.drain(start..end);
        true
    }

    /// cursor 左移 `n` 个字符, 下界 clamp 到 0.
    pub fn move_left(&mut self, n: usize) {
        self.cursor = self.cursor.saturating_sub(n);
    }

    /// cursor 右移 `n` 个字符, 上界 clamp 到 `chars().count()`.
    pub fn move_right(&mut self, n: usize) {
        let max = self.char_count();
        self.cursor = (self.cursor + n).min(max);
    }

    /// cursor 置 0 (开头)
    pub fn move_to_start(&mut self) {
        self.cursor = 0;
    }

    /// cursor 置 `chars().count()` (末尾, 即新插入字符的位置)
    pub fn move_to_end(&mut self) {
        self.cursor = self.char_count();
    }

    /// 清空 buffer 并把 cursor 归零
    pub fn clear(&mut self) {
        self.buffer.clear();
        self.cursor = 0;
    }

    /// 取 cursor 之前的字符 (渲染 caret 前的部分)
    #[must_use]
    pub fn before_cursor(&self) -> String {
        self.buffer.chars().take(self.cursor).collect()
    }

    /// 取 cursor 及之后的字符 (渲染 caret 后的部分)
    #[must_use]
    pub fn after_cursor(&self) -> String {
        self.buffer.chars().skip(self.cursor).collect()
    }

    /// 按可见宽度 `width` (列数, 含 caret 占的 1 列) 计算渲染窗口,
    /// 返回 `(caret 前可见文本, caret 后可见文本)`, 保证 caret 可见.
    ///
    /// 策略: caret 固定在窗口内 — caret 前最多占 `width - 1` 列
    /// (取 before 的尾部), 剩余列给 after 的头部. 文本超过宽度时
    /// 随光标移动横向滚动, 与终端单行输入框的通行行为一致.
    ///
    /// 注意: 按 char 计数而非显示宽度, 全角字符 (中文) 会占 2 列,
    /// 这里按 1 列算可能导致实际溢出 1 列; 测试页输入框宽度充裕,
    /// 且 ratatui 会裁剪溢出部分, 不影响正确性.
    #[must_use]
    pub fn visible_window(&self, width: usize) -> (String, String) {
        // caret 本身要占 1 列
        let Some(avail) = width.checked_sub(1) else {
            return (String::new(), String::new());
        };
        let before = self.before_cursor();
        let before_len = before.chars().count();
        let before_show = before_len.min(avail);
        // before 取尾部 before_show 个字符 (靠近 caret 的部分优先可见)
        let before_visible: String = before.chars().skip(before_len - before_show).collect();
        let after_budget = avail - before_show;
        let after_visible: String = self.after_cursor().chars().take(after_budget).collect();
        (before_visible, after_visible)
    }

    /// 渲染为单行 [`Line`]: `before` + 反色块字符 caret + `after`.
    ///
    /// `width` 是输入框内容区的可用列数 (已扣除边框). caret 用
    /// ASCII 块字符 `\u{2588}` 反色高亮 (黑字白底), 与 Settings 页
    /// 编辑态的 caret 策略一致 — 不用终端原生光标, 避免被上层
    /// `Clear` 的弹窗渲染抹掉.
    #[must_use]
    pub fn render_line(&self, width: usize) -> Line<'static> {
        let (before, after) = self.visible_window(width);
        let ed_style = Style::new().fg(Color::Black).bg(Color::White);
        Line::from_iter([
            Span::styled(before, ed_style),
            Span::styled("\u{2588}", ed_style),
            Span::styled(after, ed_style),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_delete_multibyte_chars() {
        let mut input = TextInput::new();
        input.insert_char('你');
        input.insert_char('好');
        input.insert_char('a');
        assert_eq!(input.text(), "你好a");
        assert_eq!(input.cursor(), 3);

        // 光标移到中间, 前向删除删的是光标指着的字符
        input.move_left(1);
        assert!(input.delete_forward());
        assert_eq!(input.text(), "你好");
        assert_eq!(input.cursor(), 2);

        // 中文按整字删除, 不会留下半个 UTF-8 序列
        assert!(input.delete_back());
        assert_eq!(input.text(), "你");
        assert_eq!(input.cursor(), 1);
    }

    #[test]
    fn delete_on_empty_or_boundary_is_noop() {
        let mut input = TextInput::new();
        assert!(!input.delete_back());
        assert!(!input.delete_forward());

        input.insert_char('x');
        input.move_to_start();
        assert!(!input.delete_back()); // 光标在开头, 无前可删
        assert!(input.delete_forward()); // 但可前向删除
        assert_eq!(input.text(), "");
    }

    #[test]
    fn move_cursor_clamps_and_home_end() {
        let mut input = TextInput::new();
        for c in "abc".chars() {
            input.insert_char(c);
        }
        input.move_right(10);
        assert_eq!(input.cursor(), 3);
        input.move_left(10);
        assert_eq!(input.cursor(), 0);
        input.move_to_end();
        assert_eq!(input.cursor(), 3);
        input.move_to_start();
        assert_eq!(input.cursor(), 0);
    }

    #[test]
    fn clear_resets_buffer_and_cursor() {
        let mut input = TextInput::new();
        for c in "hello".chars() {
            input.insert_char(c);
        }
        input.clear();
        assert_eq!(input.text(), "");
        assert_eq!(input.cursor(), 0);
    }

    #[test]
    fn before_after_split_at_cursor() {
        let mut input = TextInput::new();
        for c in "你好世界".chars() {
            input.insert_char(c);
        }
        input.move_left(2);
        assert_eq!(input.before_cursor(), "你好");
        assert_eq!(input.after_cursor(), "世界");
    }

    #[test]
    fn visible_window_keeps_caret_visible() {
        let mut input = TextInput::new();
        for c in "0123456789".chars() {
            input.insert_char(c);
        }
        // 宽 5: caret 占 1 列, before 最多 4 列 (取尾部), after 没预算
        let (before, after) = input.visible_window(5);
        assert_eq!(before, "6789");
        assert_eq!(after, "");

        // 光标回开头: before 空, 预算全给 after
        input.move_to_start();
        let (before, after) = input.visible_window(5);
        assert_eq!(before, "");
        assert_eq!(after, "0123");

        // 光标在中间: before 拿满后 after 拿剩余
        input.move_right(2);
        let (before, after) = input.visible_window(5);
        assert_eq!(before, "01");
        assert_eq!(after, "23");

        // 宽 0 / 1 不 panic
        assert_eq!(input.visible_window(0), (String::new(), String::new()));
        let (before, after) = input.visible_window(1);
        assert_eq!(before, "");
        assert_eq!(after, "");
    }
}
