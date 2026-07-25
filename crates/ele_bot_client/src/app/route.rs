//! 应用路由 - 右面板显示的页面
//!
//! 每个变体携带该页面需要的子状态, 这样不需要在 App 上额外维护平行的 bool/索引字段。

use super::menu::MenuItem;

/// 设备选择器选择的是输入还是输出设备
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectingKind {
    /// 选择麦克风输入设备
    Input,
    /// 选择扬声器输出设备
    Output,
}

/// 设备选择器子状态
///
/// 嵌入在 `Route::Settings::selecting` 里; overlay
/// `Overlay::DevicePicker` 持有同一实例, 二者镜像.
/// `loading` 为 true 时表示列表正在从服务端拉, Enter / ↑↓ 全部屏蔽,
/// UI 渲染一行 `<加载中...>` 占位.
#[derive(Debug, Clone)]
pub struct SelectingField {
    pub kind: SelectingKind,
    pub cursor: usize,
    pub loading: bool,
}

impl SelectingField {
    /// 新建: 输入/输出, 起始 cursor=0 (即 `<系统默认>`), 列表非空时立刻就绪.
    #[must_use] 
    pub fn new(kind: SelectingKind) -> Self {
        Self {
            kind,
            cursor: 0,
            loading: false,
        }
    }
}

/// 设备控制页面的子模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceControlMode {
    /// 显示舵机面板但焦点在侧边栏(可切换菜单)
    Idle,
    /// 右面板捕获输入(舵机控制生效)
    Active,
}

/// 设置项编辑上下文
///
/// 承担"光标定位 + buffer 修改 + 渲染字符切分"三件事; 全部按 `char`
/// (而非 byte offset) 索引, UTF-8 多字节字符视为单个编辑单位, 不会出现
/// 半个汉字的非法状态. 本类型的 API 全是 char-based 语义.
///
/// # 字段关系
/// - `cursor` 是**字符索引**, 取值范围 `0..=buffer.chars().count()`
/// - `buffer.chars().take(cursor)` 是 caret 之前的内容,
///   `buffer.chars().skip(cursor)` 是 caret 之后
/// - 调用方有责任把 `cursor` 维持在合法范围; 工具方法内部用 saturating
///   算术 + `chars().count()` 兜底, 不让 cursor 漂出去
///
/// # 例子
///
/// ```rust,ignore
/// use ele_bot_client::app::route::EditField;
/// let mut f = EditField::new(0, "示例", "你好".into(), 2);
/// f.insert_char('世');
/// assert_eq!(f.buffer, "你好世");
/// assert_eq!(f.cursor, 3);
/// ```
#[derive(Debug, Clone)]
pub struct EditField {
    pub index: usize,
    pub label: &'static str,
    pub buffer: String,
    /// 光标在 buffer 中的**字符**索引 (`0..=buffer.chars().count()`)
    pub cursor: usize,
}

impl EditField {
    /// 新建 `EditField`. 调用方传入 cursor 时 SHOULD 取
    /// `buffer.chars().count()` (末尾) 或 `0` (开头); 越界值会被方法
    /// 内部的 char/byte 转换兜底 clamp 到合法范围.
    #[must_use] 
    pub fn new(index: usize, label: &'static str, buffer: String, cursor: usize) -> Self {
        let chars = buffer.chars().count();
        Self {
            index,
            label,
            buffer,
            // clamp 越界: 输入异常仍能保持合法不变量
            cursor: cursor.min(chars),
        }
    }

    /// 光标位置 (char 索引)
    fn char_count(&self) -> usize {
        self.buffer.chars().count()
    }

    /// 在 cursor 处插入 `c`, cursor += 1. 光标在末尾时等价于 push.
    pub fn insert_char(&mut self, c: char) {
        let char_count = self.char_count();
        let cursor = self.cursor.min(char_count);
        // 把 char 索引转 byte 索引: cursor 个 char 用了多少字节
        let byte_idx = self
            .buffer
            .char_indices()
            .nth(cursor)
            .map_or(self.buffer.len(), |(b, _)| b);
        self.buffer.insert(byte_idx, c);
        self.cursor = cursor + 1;
    }

    /// 删 cursor 前一个字符 (cursor > 0 时有效). 返回是否发生了删除.
    pub fn delete_back(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        let prev = self.cursor - 1;
        // 删第 prev 个 char 的字节区间
        let start = self
            .buffer
            .char_indices()
            .nth(prev)
            .map(|(b, _)| b)
            .expect("cursor > 0 时第 cursor-1 个 char 必存在");
        let end = self
            .buffer
            .char_indices()
            .nth(self.cursor)
            .map_or(self.buffer.len(), |(b, _)| b);
        self.buffer.drain(start..end);
        self.cursor = prev;
        true
    }

    /// 删 cursor 位置字符 (cursor < `chars().count()` 时有效). 返回是否删除.
    ///
    /// 与 `delete_back` 区别: Backspace 删前面的字符, Delete 删后面的字符;
    /// 删除后 `cursor` 不变 (删的是它"指着"的那个字符).
    pub fn delete_forward(&mut self) -> bool {
        let char_count = self.char_count();
        if self.cursor >= char_count {
            return false;
        }
        let start = self
            .buffer
            .char_indices()
            .nth(self.cursor)
            .map(|(b, _)| b)
            .expect("cursor < count 时第 cursor 个 char 必存在");
        let end = self
            .buffer
            .char_indices()
            .nth(self.cursor + 1)
            .map_or(self.buffer.len(), |(b, _)| b);
        self.buffer.drain(start..end);
        true
    }

    /// cursor 左移 `n` 个字符, 下界 clamp 到 0.
    pub fn move_cursor_left(&mut self, n: usize) {
        self.cursor = self.cursor.saturating_sub(n);
    }

    /// cursor 右移 `n` 个字符, 上界 clamp 到 `chars().count()`.
    pub fn move_cursor_right(&mut self, n: usize) {
        let max = self.char_count();
        self.cursor = (self.cursor + n).min(max);
    }

    /// cursor 置 0 (开头)
    pub fn move_cursor_to_start(&mut self) {
        self.cursor = 0;
    }

    /// cursor 置 `chars().count()` (末尾, 即新插入字符的位置)
    pub fn move_cursor_to_end(&mut self) {
        self.cursor = self.char_count();
    }

    /// 取 cursor 之前的字符 (渲染 caret 前的部分)
    #[must_use] 
    pub fn before_cursor(&self) -> String {
        self.buffer.chars().take(self.cursor).collect()
    }

    /// 取 cursor 之后的字符 (渲染 caret 后的部分)
    #[must_use] 
    pub fn after_cursor(&self) -> String {
        self.buffer.chars().skip(self.cursor).collect()
    }
}

/// 右面板当前显示的页面
#[derive(Debug, Clone)]
pub enum Route {
    /// 侧边栏获得焦点。`last_entered` 记录上一次"进入"的页面,
    /// 退出子页时画面保留, 重新进入同页可恢复。
    Nav {
        last_entered: MenuItem,
    },

    DeviceControl {
        mode: DeviceControlMode,
    },

    Settings {
        selected: usize,
        editing: Option<EditField>,
        selecting: Option<SelectingField>,
    },

    LlmTest,
    TtsTest,
    About,
}

impl Route {
    /// 把当前 Route 映射回侧边栏高亮用的 `MenuItem`。
    /// `Nav` 使用 `last_entered`; 其它变体直接对应。
    #[must_use] 
    pub fn menu_item(&self) -> MenuItem {
        match self {
            Route::Nav { last_entered } => *last_entered,
            Route::DeviceControl { .. } => MenuItem::DeviceControl,
            Route::Settings { .. } => MenuItem::Settings,
            Route::LlmTest => MenuItem::LlmTest,
            Route::TtsTest => MenuItem::TtsTest,
            Route::About => MenuItem::About,
        }
    }
}

impl From<MenuItem> for Route {
    /// 创建一个"初始进入"的 Route 实例。
    /// `DeviceControl` 直接进入 Active, 用户按一次 Enter 就能调舵机;
    /// 再按一次 Enter 切到 Idle (侧边栏可重新选菜单项)。
    fn from(item: MenuItem) -> Self {
        match item {
            MenuItem::DeviceStatus => Route::Nav { last_entered: item },
            MenuItem::DeviceControl => Route::DeviceControl {
                mode: DeviceControlMode::Active,
            },
            MenuItem::LlmTest => Route::LlmTest,
            MenuItem::TtsTest => Route::TtsTest,
            MenuItem::Settings => Route::Settings {
                selected: 0,
                editing: None,
                selecting: None,
            },
            MenuItem::About => Route::About,
        }
    }
}
