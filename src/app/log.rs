use crate::app::constants::MAX_LOG_ENTRIES;
use std::collections::VecDeque;

/// 日志等级
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
}

impl LogLevel {
    /// 是否需要自动显示 popup
    pub fn should_show_popup(self) -> bool {
        matches!(self, LogLevel::Warning | LogLevel::Error)
    }

    /// 获取显示颜色
    pub fn color(self) -> ratatui::style::Color {
        match self {
            LogLevel::Info => ratatui::style::Color::Green,
            LogLevel::Warning => ratatui::style::Color::Yellow,
            LogLevel::Error => ratatui::style::Color::Red,
        }
    }

    /// 获取前缀
    pub fn prefix(self) -> &'static str {
        match self {
            LogLevel::Info => "[INFO]",
            LogLevel::Warning => "[WARN]",
            LogLevel::Error => "[ERROR]",
        }
    }
}

/// 日志条目
#[derive(Clone, Debug)]
pub struct LogEntry {
    pub message: String,
    pub count: u32,
    pub timestamp: String,
    pub level: LogLevel,
}

impl LogEntry {
    pub fn new(message: String, level: LogLevel) -> Self {
        Self {
            message,
            count: 1,
            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
            level,
        }
    }

    pub fn with_count(&self) -> String {
        if self.count > 1 {
            format!("{} (x{})", self.message, self.count)
        } else {
            self.message.clone()
        }
    }
}

/// 日志队列
#[derive(Clone)]
pub struct LogQueue {
    entries: VecDeque<LogEntry>,
    last_message: Option<(String, LogLevel)>, // (消息, 等级)
    has_new_important: bool,                  // 是否有新的 warning/error
}

impl LogQueue {
    pub fn new() -> Self {
        Self {
            entries: VecDeque::with_capacity(MAX_LOG_ENTRIES),
            last_message: None,
            has_new_important: false,
        }
    }

    pub fn add(&mut self, message: String, level: LogLevel) {
        // 尝试合并相同消息
        if let Some((last_msg, last_level)) = &self.last_message {
            if last_msg == &message && last_level == &level {
                if let Some(last_entry) = self.entries.back_mut() {
                    last_entry.count += 1;
                    // 更新重要标记
                    if level.should_show_popup() {
                        self.has_new_important = true;
                    }
                    return;
                }
            }
        }

        self.entries
            .push_back(LogEntry::new(message.clone(), level));
        self.last_message = Some((message, level));

        // 如果是重要日志，标记
        if level.should_show_popup() {
            self.has_new_important = true;
        }

        // 保持容量限制
        while self.entries.len() > MAX_LOG_ENTRIES {
            self.entries.pop_front();
            if self.entries.is_empty() {
                self.last_message = None;
            } else {
                let back = self.entries.back().unwrap();
                self.last_message = Some((back.message.clone(), back.level));
            }
        }
    }

    /// 添加 Info 日志
    pub fn info(&mut self, message: String) {
        self.add(message, LogLevel::Info);
    }

    /// 添加 Warning 日志
    pub fn warn(&mut self, message: String) {
        self.add(message, LogLevel::Warning);
    }

    /// 添加 Error 日志
    pub fn error(&mut self, message: String) {
        self.add(message, LogLevel::Error);
    }

    pub fn entries(&self) -> Vec<LogEntry> {
        self.entries.iter().cloned().collect()
    }

    /// 是否有新的重要日志 (warning/error)
    pub fn has_new_important(&self) -> bool {
        self.has_new_important
    }

    /// 清除重要日志标记
    pub fn clear_important_flag(&mut self) {
        self.has_new_important = false;
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.last_message = None;
        self.has_new_important = false;
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

impl Default for LogQueue {
    fn default() -> Self {
        Self::new()
    }
}
