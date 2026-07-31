//! 系统状态采集
//!
//! 定时采集 SoC 温度 / CPU 占用 / 内存, 通过事件总线广播
//! `ServerEvent::SystemStats`, WS 订阅任务直接外发给客户端.
//!
//! 平台支持:
//! - CPU / 内存: sysinfo, 跨平台 (Linux / macOS / Windows)
//! - SoC 温度: 仅 Linux thermal zone (RK3566 为 `soc-thermal`),
//!   其它平台恒为 `None`

use std::time::Duration;

use ele_bot_proto::{ServerEvent, SystemStatsDto};
use sysinfo::System;

use crate::event_bus::{BusEvent, EventBus};

/// 采集间隔
const INTERVAL: Duration = Duration::from_secs(2);

/// 启动采集任务. 调用后立即返回, 后台 tokio task 周期性 publish.
pub fn spawn(bus: EventBus) {
    tokio::spawn(async move {
        let mut sys = System::new();
        // thermal zone 路径解析一次后缓存, 避免每周期扫目录
        let mut temp_zone: Option<Option<std::path::PathBuf>> = None;
        let mut ticker = tokio::time::interval(INTERVAL);
        loop {
            ticker.tick().await;
            sys.refresh_cpu_usage();
            sys.refresh_memory();
            let zone = temp_zone.get_or_insert_with(find_soc_thermal_zone);
            let stats = SystemStatsDto {
                soc_temp_c: zone.as_ref().and_then(|z| read_temp_c(z)),
                cpu_usage: sys.global_cpu_usage(),
                mem_used_mb: sys.used_memory() / 1024 / 1024,
                mem_total_mb: sys.total_memory() / 1024 / 1024,
            };
            bus.publish(BusEvent::ServerEvent(ServerEvent::SystemStats { stats }));
        }
    });
}

/// 在 `/sys/class/thermal` 里找 SoC 对应的 thermal zone.
///
/// 优先 type 含 "soc" 的 zone (RK3566 是 `soc-thermal`), 找不到退到
/// 第一个可读的 zone; 非 Linux / 无 thermal zone 时返回 `None`.
#[cfg(target_os = "linux")]
fn find_soc_thermal_zone() -> Option<std::path::PathBuf> {
    let base = std::path::Path::new("/sys/class/thermal");
    let mut first: Option<std::path::PathBuf> = None;
    for entry in std::fs::read_dir(base).ok()?.flatten() {
        let dir = entry.path();
        if !dir.join("temp").exists() {
            continue;
        }
        let zone_type = std::fs::read_to_string(dir.join("type")).unwrap_or_default();
        if first.is_none() {
            first = Some(dir.clone());
        }
        if zone_type.to_ascii_lowercase().contains("soc") {
            return Some(dir);
        }
    }
    first
}

#[cfg(not(target_os = "linux"))]
fn find_soc_thermal_zone() -> Option<std::path::PathBuf> {
    None
}

/// 读 thermal zone 温度. zone 的 `temp` 文件单位是毫摄氏度.
#[cfg(target_os = "linux")]
fn read_temp_c(zone: &std::path::Path) -> Option<f32> {
    let raw = std::fs::read_to_string(zone.join("temp")).ok()?;
    let milli: f32 = raw.trim().parse().ok()?;
    Some(milli / 1000.0)
}

#[cfg(not(target_os = "linux"))]
fn read_temp_c(_zone: &std::path::Path) -> Option<f32> {
    None
}
