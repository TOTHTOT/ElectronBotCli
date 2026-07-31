use crate::app::App;
use crate::ui_components::create_block;
use ratatui::{
    prelude::*,
    widgets::{Cell, Paragraph, Row, Table},
};

/// 音量条宽度 — 总共 20 个字符宽, 含两端的方括号
const VOLUME_BAR_WIDTH: usize = 20;

fn status_color(ok: bool) -> Color {
    if ok {
        Color::Green
    } else {
        Color::Red
    }
}

/// 把 0..=100 的音量渲染成 `[█████│---------]` 形状的字符串
///
/// - 始终以 `[` `]` 边界包住, 0 时仍可见音量条形态
/// - 用 `│` 作"游标", 标识当前音量位置 (即便 0 也保留 1 个 `│` 在最左)
/// - 字符用 `█` (满) + `─` (空), 满部分 Cyan, 游标 Yellow
fn render_volume_bar(volume: i32) -> String {
    render_bar(volume)
}

/// 通用百分比条, 与音量条同形态 (0..=100 截断)
fn render_bar(pct: i32) -> String {
    let v = pct.clamp(0, 100) as usize;
    // 内宽 = 总宽 - 2 (左右括号)
    let inner = VOLUME_BAR_WIDTH - 2;
    // 满格数, 至少 1 个 `█` (音量 > 0) 或 0 个 (音量 = 0)
    let filled = if v > 0 { (v * inner / 100).max(1) } else { 0 };
    // 游标位置: 满格末. 当 v=0 时, 游标在 0 处
    let cursor_pos = if v > 0 { filled - 1 } else { 0 };
    let mut bar = String::with_capacity(VOLUME_BAR_WIDTH);
    bar.push('[');
    for i in 0..inner {
        if i < filled.saturating_sub(1) {
            bar.push('█');
        } else if i == cursor_pos {
            bar.push('│');
        } else {
            bar.push('─');
        }
    }
    bar.push(']');
    bar
}

pub fn render(frame: &mut Frame, area: Rect, app: &App, border_color: Color) {
    // 抓服务端镜像快照, 锁立刻释放, 避免 render 期间持锁
    let (is_connected, network_label, volume, sys_stats) = {
        let server = app.server.lock().unwrap();
        (
            server.robot_connected,
            if server.net_connected {
                "已连接"
            } else {
                "未连接"
            },
            server.volume,
            server.sys_stats,
        )
    };
    let battery: u32 = 85; // TODO: 后续获取真实电量

    // 音量条本体
    let volume_bar = render_volume_bar(volume);
    // 音量文字描述: 0 静音 / 1-30 小声 / 31-60 中等 / 61-100 大声
    let volume_label = match volume {
        0 => "静音",
        1..=30 => "小声",
        31..=60 => "中等",
        _ => "大声",
    };

    // 系统状态三行: 未收到推送时统一显示 "--"
    let (temp_text, temp_ok, cpu_text, mem_text) = match sys_stats {
        Some(s) => {
            let temp = s
                .soc_temp_c
                .map(|t| format!("{t:.1}°C"))
                .unwrap_or_else(|| "--".into());
            let temp_ok = s.soc_temp_c.is_none_or(|t| t < 80.0);
            let cpu_bar = render_bar(s.cpu_usage as i32);
            let mem_pct = if s.mem_total_mb > 0 {
                (s.mem_used_mb * 100 / s.mem_total_mb) as i32
            } else {
                0
            };
            let mem_bar = render_bar(mem_pct);
            (
                temp,
                temp_ok,
                format!("{cpu_bar}  {:.1}%", s.cpu_usage),
                format!("{mem_bar}  {} / {} MiB", s.mem_used_mb, s.mem_total_mb),
            )
        }
        None => ("--".into(), true, "--".into(), "--".into()),
    };

    // 使用 Table 实现网格布局
    let table = Table::new(
        vec![
            Row::new(vec![
                Cell::from(Span::styled("连接状态", Style::new().fg(Color::Yellow))),
                Cell::from(Span::styled(
                    if is_connected {
                        "已连接"
                    } else {
                        "未连接"
                    },
                    Style::new().fg(status_color(is_connected)).bold(),
                )),
            ]),
            Row::new(vec![
                Cell::from(Span::styled("上位机电量", Style::new().fg(Color::Yellow))),
                Cell::from(Span::styled(
                    format!("{battery}%"),
                    Style::new().fg(status_color(battery > 50)),
                )),
            ]),
            Row::new(vec![
                Cell::from(Span::styled("网络状态", Style::new().fg(Color::Yellow))),
                Cell::from(Span::styled(
                    network_label,
                    Style::new().fg(status_color(network_label == "已连接")),
                )),
            ]),
            Row::new(vec![
                Cell::from(Span::styled("输入音量", Style::new().fg(Color::Yellow))),
                // 音量条 + 数字 + 文字描述, 同一行紧凑展示
                Cell::from(Span::styled(
                    format!("{volume_bar}  {volume} ({volume_label})"),
                    Style::new().fg(Color::Cyan),
                )),
            ]),
            Row::new(vec![
                Cell::from(Span::styled("SoC 温度", Style::new().fg(Color::Yellow))),
                Cell::from(Span::styled(
                    temp_text,
                    Style::new().fg(status_color(temp_ok)),
                )),
            ]),
            Row::new(vec![
                Cell::from(Span::styled("CPU 占用", Style::new().fg(Color::Yellow))),
                Cell::from(Span::styled(cpu_text, Style::new().fg(Color::Cyan))),
            ]),
            Row::new(vec![
                Cell::from(Span::styled("内存", Style::new().fg(Color::Yellow))),
                Cell::from(Span::styled(mem_text, Style::new().fg(Color::Cyan))),
            ]),
        ],
        &[Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)],
    )
    .column_spacing(2);
    let outer_block = create_block("操作说明".to_string(), border_color, border_color);
    let inner_area = outer_block.inner(area);
    frame.render_widget(outer_block, area);

    let widget = Paragraph::new(Line::raw("")).alignment(Alignment::Left);
    frame.render_widget(widget, inner_area);

    let inner = area.inner(Margin {
        horizontal: 1,
        vertical: 1,
    });
    frame.render_widget(table, inner);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_bar_zero_has_shape() {
        // 0 时仍然是 `[│─────────────────]` 形态, 不退化成纯横线
        let bar = render_volume_bar(0);
        assert!(bar.starts_with('['));
        assert!(bar.ends_with(']'));
        assert!(bar.contains('│'));
        assert_eq!(bar.chars().count(), VOLUME_BAR_WIDTH);
    }

    #[test]
    fn volume_bar_full_filled() {
        let bar = render_volume_bar(100);
        assert!(bar.starts_with('['));
        assert!(bar.ends_with(']'));
        // 100 时内宽 18 个 `█` 加 1 个游标 `│`, 无 `─`
        assert!(!bar.contains('─'));
    }

    #[test]
    fn volume_bar_clamped() {
        // 越界值截到合法范围
        assert_eq!(render_volume_bar(-5), render_volume_bar(0));
        assert_eq!(render_volume_bar(150), render_volume_bar(100));
    }
}
