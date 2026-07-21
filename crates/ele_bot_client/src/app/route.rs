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
#[derive(Debug, Clone)]
pub struct EditField {
    pub index: usize,
    pub label: &'static str,
    pub buffer: String,
}

impl EditField {
    pub fn new(index: usize, label: &'static str, buffer: String) -> Self {
        Self {
            index,
            label,
            buffer,
        }
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
    /// 把当前 Route 映射回侧边栏高亮用的 MenuItem。
    /// `Nav` 使用 `last_entered`; 其它变体直接对应。
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
    /// DeviceControl 直接进入 Active, 用户按一次 Enter 就能调舵机;
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
