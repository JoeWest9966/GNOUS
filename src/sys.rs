//! Read-only Linux system probes backing the overview page.
//!
//! Everything comes from procfs / sysfs (plus `statvfs` and `localtime_r`), so a
//! sample costs microseconds and can run on the UI thread once per second.

use std::ffi::CString;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// One frame of real machine state, already formatted for the overview layout.
pub struct Snapshot {
    pub hostname: String,
    /// `GNOME 48 · Wayland`
    pub session: String,
    /// `Wayland`
    pub session_kind: String,
    pub headline: &'static str,
    pub detail: String,
    pub healthy: bool,
    pub clock: String,

    pub cpu_value: String,
    pub cpu_left: String,
    pub cpu_right: String,
    pub cpu_fill: f32,

    pub mem_value: String,
    pub mem_unit: String,
    pub mem_left: String,
    pub mem_right: String,
    pub mem_fill: f32,

    pub disk_value: String,
    pub disk_unit: String,
    pub disk_left: String,
    pub disk_right: String,
    pub disk_fill: f32,

    pub net_value: String,
    pub net_unit: String,
    pub net_left: String,
    pub net_right: String,
    pub net_fill: f32,
}

/// Samples the machine. Cheap to construct once, then [`Probe::sample`] each
/// tick: static facts (hostname, session, core count, RAM size) are read once,
/// counters keep the previous reading so CPU and network become rates.
pub struct Probe {
    hostname: String,
    session: String,
    session_kind: String,
    cpu_cores: usize,
    cpu_ghz: f32,
    mem_total_kb: u64,
    disk_label: &'static str,
    temp_path: Option<PathBuf>,
    last_cpu: Option<(u64, u64)>,
    last_net: Option<(u64, u64, Instant)>,
    net_rate: (f64, f64),
}

impl Probe {
    pub fn new() -> Self {
        let hostname = read("/proc/sys/kernel/hostname")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "localhost".into());
        let (session, session_kind) = session_label();
        let cpu_cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        let cpu_ghz = cpu_mhz().map(|mhz| mhz / 1000.).unwrap_or(0.);
        let mem_total_kb = mem_total_kb();
        let disk_label = disk_label();
        let temp_path = temp_sensor();

        let mut probe = Self {
            hostname,
            session,
            session_kind,
            cpu_cores,
            cpu_ghz,
            mem_total_kb,
            disk_label,
            temp_path,
            last_cpu: cpu_times(),
            last_net: None,
            net_rate: (0., 0.),
        };
        // First rates need a sampling window; 120 ms is enough for a readable
        // CPU number and imperceptible at startup.
        std::thread::sleep(Duration::from_millis(120));
        probe.last_net = net_bytes().map(|(rx, tx)| (rx, tx, Instant::now()));
        probe
    }

    pub fn sample(&mut self) -> Snapshot {
        let (cpu_pct, cpu_fill) = self.cpu();

        let (mem_used_kb, mem_fill) = mem_used_kb(self.mem_total_kb);
        let mem_total_gb = self.mem_total_kb as f64 / 1024. / 1024.;
        let swap_used_kb = swap_used_kb();

        let (disk_used, disk_total) = disk_usage("/").unwrap_or((0, 0));
        let disk_fill = if disk_total > 0 {
            disk_used as f32 / disk_total as f32
        } else {
            0.
        };
        let disk_free_gb = (disk_total.saturating_sub(disk_used)) as f64 / 1e9;

        let (down, up) = self.net_rate();
        let temp = self.temperature();

        let headline = if disk_fill >= 0.9 {
            "存储空间告急"
        } else if mem_fill >= 0.9 {
            "内存压力偏高"
        } else if temp.is_some_and(|t| t >= 85.) {
            "散热需要关注"
        } else {
            "系统运行正常"
        };

        Snapshot {
            hostname: self.hostname.clone(),
            session: self.session.clone(),
            session_kind: self.session_kind.clone(),
            headline,
            healthy: headline == "系统运行正常",
            clock: local_clock(),
            detail: format!("已连续运行 {} · 每 1 秒自动刷新", uptime_label()),

            cpu_value: format!("{cpu_pct}"),
            cpu_left: format!(
                "{} 线程{}",
                self.cpu_cores,
                if self.cpu_ghz >= 0.1 {
                    format!(" · {:.1} GHz", self.cpu_ghz)
                } else {
                    String::new()
                }
            ),
            cpu_right: temp
                .map(|t| format!("{t:.0}°C"))
                .unwrap_or_else(|| "—".into()),
            cpu_fill,

            mem_value: format!("{:.1}", mem_used_kb as f64 / 1024. / 1024.),
            mem_unit: format!("/ {mem_total_gb:.0} GB"),
            mem_left: if swap_used_kb > 0 {
                format!("SWAP {:.1} GB", swap_used_kb as f64 / 1024. / 1024.)
            } else {
                "无 SWAP".into()
            },
            mem_right: format!("{:.0}%", mem_fill * 100.),
            mem_fill,

            disk_value: format!("{:.0}", disk_used as f64 / 1e9),
            disk_unit: format!("/ {:.0} GB", disk_total as f64 / 1e9),
            disk_left: self.disk_label.into(),
            disk_right: format!("剩余 {disk_free_gb:.0} GB"),
            disk_fill,

            net_value: format!("{down:.1}"),
            net_unit: "MB/s".into(),
            net_left: self.net_iface_label(),
            net_right: format!("↓ {down:.1} ↑ {up:.1}"),
            net_fill: (down as f32 / 20.).clamp(0., 1.),
        }
    }

    fn cpu(&mut self) -> (u32, f32) {
        let Some(now) = cpu_times() else {
            return (0, 0.);
        };
        let Some((ptotal, pidle)) = self.last_cpu.replace(now) else {
            return (0, 0.);
        };
        let (total, idle) = now;
        let dt = total.saturating_sub(ptotal);
        let di = idle.saturating_sub(pidle);
        if dt == 0 {
            return (0, 0.);
        }
        let fill = (dt.saturating_sub(di)) as f32 / dt as f32;
        ((fill * 100.).round() as u32, fill)
    }

    fn net_rate(&mut self) -> (f64, f64) {
        if let Some((rx, tx)) = net_bytes() {
            let now = Instant::now();
            if let Some((prx, ptx, at)) = self.last_net {
                let secs = now.duration_since(at).as_secs_f64();
                if secs >= 0.05 {
                    self.net_rate = (
                        rx.saturating_sub(prx) as f64 / secs / 1e6,
                        tx.saturating_sub(ptx) as f64 / secs / 1e6,
                    );
                    self.last_net = Some((rx, tx, now));
                }
            } else {
                self.last_net = Some((rx, tx, now));
            }
        }
        self.net_rate
    }

    fn net_iface_label(&self) -> String {
        let iface = net_iface().unwrap_or_default();
        if iface.starts_with("wl") {
            "Wi-Fi".into()
        } else if iface.starts_with("en") || iface.starts_with("eth") {
            "以太网".into()
        } else if iface.is_empty() {
            "无连接".into()
        } else {
            iface
        }
    }

    fn temperature(&self) -> Option<f32> {
        let path = self.temp_path.as_ref()?;
        let raw: f64 = read(path)?.trim().parse().ok()?;
        let celsius = if raw > 1000. { raw / 1000. } else { raw };
        (0. ..=125.).contains(&celsius).then_some(celsius as f32)
    }
}

// ============================================================================
// probes
// ============================================================================

fn read(path: impl AsRef<std::path::Path>) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// `(total, idle)` jiffies from the aggregate `cpu` line.
fn cpu_times() -> Option<(u64, u64)> {
    let stat = read("/proc/stat")?;
    let line = stat.lines().next()?;
    let fields: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|f| f.parse().ok())
        .collect();
    if fields.len() < 4 {
        return None;
    }
    let total: u64 = fields.iter().sum();
    let idle = fields[3] + fields.get(4).copied().unwrap_or(0);
    Some((total, idle))
}

fn cpu_mhz() -> Option<f32> {
    let info = read("/proc/cpuinfo")?;
    let mut sum = 0.;
    let mut n = 0.;
    for line in info.lines() {
        if let Some(v) = line.strip_prefix("cpu MHz") {
            if let Some(mhz) = v
                .split(':')
                .nth(1)
                .and_then(|v| v.trim().parse::<f32>().ok())
            {
                sum += mhz;
                n += 1.;
            }
        }
    }
    (n > 0.).then(|| sum / n)
}

fn mem_total_kb() -> u64 {
    meminfo_kb("MemTotal").unwrap_or(0)
}

fn mem_used_kb(total: u64) -> (u64, f32) {
    let available = meminfo_kb("MemAvailable").unwrap_or(total);
    let used = total.saturating_sub(available);
    let fill = if total > 0 {
        used as f32 / total as f32
    } else {
        0.
    };
    (used, fill)
}

fn swap_used_kb() -> u64 {
    let total = meminfo_kb("SwapTotal").unwrap_or(0);
    let free = meminfo_kb("SwapFree").unwrap_or(0);
    total.saturating_sub(free)
}

fn meminfo_kb(key: &str) -> Option<u64> {
    let info = read("/proc/meminfo")?;
    info.lines()
        .find(|l| l.starts_with(key) && l.as_bytes().get(key.len()) == Some(&b':'))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
}

/// `(used, total)` bytes for the filesystem holding `path`.
fn disk_usage(path: &str) -> Option<(u64, u64)> {
    let c_path = CString::new(path).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c_path.as_ptr(), &mut st) } != 0 {
        return None;
    }
    let block = if st.f_frsize > 0 {
        st.f_frsize
    } else {
        st.f_bsize
    } as u64;
    let total = st.f_blocks as u64 * block;
    let free = st.f_bavail as u64 * block;
    Some((total.saturating_sub(free), total))
}

/// `EXT4 · NVMe` style label for the root filesystem.
fn disk_label() -> &'static str {
    let mounts = read("/proc/mounts").unwrap_or_default();
    let (device, fstype) = mounts
        .lines()
        .find_map(|l| {
            let mut f = l.split_whitespace();
            let device = f.next()?;
            let mount = f.next()?;
            let fstype = f.next()?;
            (mount == "/").then(|| (device.to_string(), fstype.to_string()))
        })
        .unwrap_or_default();
    let fs = match fstype.as_str() {
        "ext4" | "ext3" => "EXT4",
        "btrfs" => "Btrfs",
        "xfs" => "XFS",
        "zfs" => "ZFS",
        "f2fs" => "F2FS",
        _ => "DISK",
    };
    let medium = if device.contains("nvme") {
        "NVMe"
    } else if device.starts_with("/dev/sd") {
        "SSD"
    } else {
        "本地"
    };
    // The layout prints this as a static label, so keep the set small.
    match (fs, medium) {
        ("EXT4", "NVMe") => "EXT4 · NVMe",
        ("EXT4", _) => "EXT4 · SSD",
        ("Btrfs", "NVMe") => "Btrfs · NVMe",
        ("Btrfs", _) => "Btrfs · SSD",
        ("XFS", _) => "XFS · 本地",
        ("ZFS", _) => "ZFS · 本地",
        ("F2FS", _) => "F2FS · 本地",
        _ => "本地磁盘",
    }
}

/// Total received / transmitted bytes over all non-loopback interfaces.
fn net_bytes() -> Option<(u64, u64)> {
    let dev = read("/proc/net/dev")?;
    let mut rx = 0u64;
    let mut tx = 0u64;
    for line in dev.lines().skip(2) {
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        if name.trim() == "lo" {
            continue;
        }
        let fields: Vec<&str> = rest.split_whitespace().collect();
        if fields.len() < 9 {
            continue;
        }
        rx += fields[0].parse::<u64>().unwrap_or(0);
        tx += fields[8].parse::<u64>().unwrap_or(0);
    }
    Some((rx, tx))
}

fn net_iface() -> Option<String> {
    let dev = read("/proc/net/dev")?;
    dev.lines().skip(2).find_map(|line| {
        let (name, rest) = line.split_once(':')?;
        let name = name.trim();
        if name == "lo" {
            return None;
        }
        let fields: Vec<&str> = rest.split_whitespace().collect();
        let traffic: u64 = fields.first().and_then(|v| v.parse().ok()).unwrap_or(0);
        (traffic > 0).then(|| name.to_string())
    })
}

/// First thermal zone reporting a plausible package temperature.
fn temp_sensor() -> Option<PathBuf> {
    let entries = std::fs::read_dir("/sys/class/thermal").ok()?;
    let mut zones: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("thermal_zone"))
        })
        .collect();
    zones.sort();
    zones.into_iter().find_map(|zone| {
        let t = zone.join("temp");
        let raw: f64 = read(&t)?.trim().parse().ok()?;
        let celsius = if raw > 1000. { raw / 1000. } else { raw };
        (0. ..=125.).contains(&celsius).then_some(t)
    })
}

fn uptime_label() -> String {
    let secs = read("/proc/uptime")
        .and_then(|s| s.split_whitespace().next()?.parse::<f64>().ok())
        .unwrap_or(0.);
    let total = secs as u64;
    let days = total / 86_400;
    let hours = (total % 86_400) / 3_600;
    let minutes = (total % 3_600) / 60;
    match (days, hours) {
        (0, 0) => format!("{minutes} 分"),
        (0, _) => format!("{hours} 小时 {minutes} 分"),
        (_, 0) => format!("{days} 天"),
        _ => format!("{days} 天 {hours} 小时"),
    }
}

pub fn local_clock() -> String {
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&now, &mut tm).is_null() {
            return "--:--".into();
        }
        format!("{:02}:{:02}", tm.tm_hour, tm.tm_min)
    }
}

/// `GNOME 48 · Wayland`, falling back to whatever the session advertises.
fn session_label() -> (String, String) {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let kind = match std::env::var("XDG_SESSION_TYPE")
        .unwrap_or_default()
        .to_lowercase()
        .as_str()
    {
        "wayland" => "Wayland",
        "x11" => "X11",
        "tty" => "TTY",
        _ => "Session",
    };
    let desktop = match desktop.split(':').next().unwrap_or("") {
        "GNOME" | "ubuntu" => "GNOME",
        "KDE" => "Plasma",
        "XFCE" => "XFCE",
        "sway" => "Sway",
        other if !other.is_empty() => other,
        _ => "Desktop",
    };
    let full = match (desktop, desktop_version()) {
        ("GNOME", Some(v)) => format!("GNOME {v} · {kind}"),
        (name, _) => format!("{name} · {kind}"),
    };
    (full, kind.to_string())
}

fn desktop_version() -> Option<String> {
    // Distributions that ship the gnome meta-package write this file.
    if let Some(xml) = read("/usr/share/gnome/gnome-version.xml") {
        let major = xml
            .split("<major>")
            .nth(1)
            .and_then(|s| s.split("</major>").next())
            .map(str::trim);
        if let Some(major) = major.filter(|m| !m.is_empty()) {
            return Some(major.to_string());
        }
    }
    // Otherwise ask dpkg; costs one file read and avoids running gnome-shell.
    let status = read("/var/lib/dpkg/status")?;
    for block in status.split("\n\n") {
        if !block.lines().any(|l| l.trim() == "Package: gnome-shell") {
            continue;
        }
        for line in block.lines() {
            if let Some(version) = line.strip_prefix("Version: ") {
                let major = version.split('.').next()?;
                if !major.is_empty() {
                    return Some(major.to_string());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_reports_plausible_values() {
        let mut probe = Probe::new();
        let s = probe.sample();
        assert!(!s.hostname.is_empty(), "hostname");
        assert!(s.session.contains('·'), "session: {}", s.session);
        assert!(
            s.mem_fill >= 0. && s.mem_fill <= 1.,
            "mem_fill: {}",
            s.mem_fill
        );
        assert!(
            s.disk_fill > 0. && s.disk_fill <= 1.,
            "disk_fill: {}",
            s.disk_fill
        );
        assert!(
            s.disk_value.parse::<f64>().is_ok(),
            "disk_value: {}",
            s.disk_value
        );
        assert!(s.clock.len() == 5, "clock: {}", s.clock);
        assert!(s.detail.contains('·'), "detail: {}", s.detail);
        let second = probe.sample();
        assert!(
            second.cpu_value.parse::<u32>().is_ok(),
            "cpu: {}",
            second.cpu_value
        );
    }

    #[test]
    fn uptime_is_formatted() {
        let label = uptime_label();
        assert!(
            label.ends_with('分') || label.ends_with("小时") || label.ends_with('天'),
            "{label}"
        );
    }
}
