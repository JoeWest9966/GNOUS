//! Process sampling for the `processes` plugin.
//!
//! Everything is read from procfs: `/proc/<pid>/stat` for CPU ticks and the
//! parent link, `/proc/<pid>/status` for owner/RSS/threads. CPU percentages are
//! deltas between two samples, scaled per core like `top`, so a busy process on
//! a 12-thread machine can read above 100%.
//!
//! Sampling is cheap (two small files per process) and runs on the UI thread
//! once a second; commands and cgroups are read lazily when a view needs them.

use std::collections::HashMap;

/// Process state letter (`R`, `S`, `I`, `D`, ...) normalized for display.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Running,
    Sleeping,
    Idle,
    IoWait,
    Stopped,
    Zombie,
    Other,
}

impl State {
    fn from_letter(letter: &str) -> Self {
        match letter {
            "R" => State::Running,
            "S" => State::Sleeping,
            "I" => State::Idle,
            "D" => State::IoWait,
            "T" | "t" => State::Stopped,
            "Z" => State::Zombie,
            _ => State::Other,
        }
    }

    /// Upper-case label, as shown in the table.
    pub fn label(self) -> &'static str {
        match self {
            State::Running => "RUNNING",
            State::Sleeping => "SLEEP",
            State::Idle => "IDLE",
            State::IoWait => "IO WAIT",
            State::Stopped => "STOPPED",
            State::Zombie => "ZOMBIE",
            State::Other => "OTHER",
        }
    }

    /// CSS-ish token used by the UI to pick a color.
    pub fn kind(self) -> &'static str {
        match self {
            State::Running => "running",
            State::Sleeping => "sleep",
            State::Idle => "idle",
            State::IoWait => "wait",
            State::Stopped | State::Zombie => "wait",
            State::Other => "sleep",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Process {
    pub pid: u32,
    pub ppid: u32,
    pub name: String,
    pub user: String,
    #[allow(dead_code)]
    pub uid: u32,
    pub state: State,
    /// Per-core CPU percentage over the last sampling window.
    pub cpu: f32,
    pub rss_kb: u64,
    pub threads: u32,
    /// Seconds since boot when the process started.
    pub age_secs: u64,
    /// Recent CPU samples, newest last (only kept for the busiest processes).
    pub history: Vec<u8>,
}

impl Process {
    pub fn rss_mb(&self) -> f32 {
        self.rss_kb as f32 / 1024.
    }

    pub fn mem_label(&self) -> String {
        let mb = self.rss_mb();
        if mb >= 1024. {
            format!("{:.1} GB", mb / 1024.)
        } else {
            format!("{mb:.0} MB")
        }
    }

    pub fn age_label(&self) -> String {
        let secs = self.age_secs;
        match (secs / 86_400, (secs % 86_400) / 3_600, (secs % 3_600) / 60) {
            (0, 0, m) => format!("{m} 分钟前"),
            (0, h, _) => format!("{h} 小时前"),
            (d, _, _) => format!("{d} 天前"),
        }
    }
}

pub struct Snapshot {
    pub processes: Vec<Process>,
    pub running: usize,
    pub threads: u32,
    pub cpu_total: f32,
    pub mem_used_kb: u64,
    pub mem_total_kb: u64,
    /// `1 分钟` / `5 分钟` load averages.
    pub load: (f32, f32),
}

pub struct Sampler {
    prev_ticks: HashMap<u32, u64>,
    prev_total: u64,
    cores: f32,
    clock_ticks: f64,
    users: HashMap<u32, String>,
    history: HashMap<u32, Vec<u8>>,
    first: bool,
}

const HISTORY_LEN: usize = 30;
const HISTORY_TOP: usize = 12;

impl Sampler {
    pub fn new() -> Self {
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1) as f32;
        let clock_ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
        Self {
            prev_ticks: HashMap::new(),
            prev_total: 0,
            cores,
            clock_ticks: if clock_ticks > 0 {
                clock_ticks as f64
            } else {
                100.
            },
            users: HashMap::new(),
            history: HashMap::new(),
            first: true,
        }
    }

    pub fn sample(&mut self) -> Snapshot {
        let (total, idle) = cpu_times().unwrap_or((0, 0));
        let delta_total = if self.first {
            1
        } else {
            total.saturating_sub(self.prev_total).max(1)
        };

        let mut processes = Vec::new();
        let mut next_ticks = HashMap::with_capacity(self.prev_ticks.len());
        let mut running = 0;
        let mut threads = 0;

        if let Ok(entries) = std::fs::read_dir("/proc") {
            for entry in entries.flatten() {
                let Some(pid) = entry
                    .file_name()
                    .to_str()
                    .and_then(|name| name.parse::<u32>().ok())
                else {
                    continue;
                };
                let Some(mut process) = read_process(pid, &mut self.users, self.clock_ticks) else {
                    continue;
                };
                let ticks = process.ticks;
                let known = !self.first && self.prev_ticks.contains_key(&pid);
                let delta = ticks.saturating_sub(*self.prev_ticks.get(&pid).unwrap_or(&ticks));
                process.process.cpu = if known {
                    delta as f32 / delta_total as f32 * self.cores * 100.
                } else {
                    0.
                };
                if process.process.state == State::Running {
                    running += 1;
                }
                threads += process.process.threads;
                next_ticks.insert(pid, ticks);
                processes.push(process.process);
            }
        }

        self.prev_ticks = next_ticks;
        self.prev_total = total;
        self.first = false;

        processes.sort_by(|a, b| {
            b.cpu
                .partial_cmp(&a.cpu)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.rss_kb.cmp(&a.rss_kb))
        });

        self.track_history(&mut processes);

        let (mem_total_kb, mem_used_kb) = memory();
        let cpu_total = if total > 0 {
            (1. - idle as f32 / total as f32).max(0.) * 100.
        } else {
            0.
        };

        Snapshot {
            processes,
            running,
            threads,
            cpu_total,
            mem_used_kb,
            mem_total_kb,
            load: load_average(),
        }
    }

    /// Keep a 30-point CPU sparkline for the busiest processes only.
    fn track_history(&mut self, processes: &mut [Process]) {
        let live: Vec<u32> = processes.iter().map(|p| p.pid).collect();
        self.history.retain(|pid, _| live.contains(pid));
        for process in processes.iter_mut().take(HISTORY_TOP) {
            let series = self.history.entry(process.pid).or_default();
            series.push(process.cpu.clamp(0., 100.) as u8);
            if series.len() > HISTORY_LEN {
                let excess = series.len() - HISTORY_LEN;
                series.drain(0..excess);
            }
            process.history = series.clone();
        }
    }
}

// ============================================================================
// procfs readers
// ============================================================================

struct Raw {
    process: Process,
    ticks: u64,
}

fn read_process(pid: u32, users: &mut HashMap<u32, String>, clock_ticks: f64) -> Option<Raw> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let open = stat.find('(')?;
    let close = stat.rfind(')')?;
    let name = stat[open + 1..close].to_string();
    let fields: Vec<&str> = stat[close + 1..].split_whitespace().collect();
    // fields[0] = state (field 3 of /proc/pid/stat)
    let state = State::from_letter(fields.first()?);
    let ppid: u32 = fields.get(1)?.parse().ok()?;
    let utime: u64 = fields.get(11)?.parse().ok()?;
    let stime: u64 = fields.get(12)?.parse().ok()?;
    let threads: u32 = fields.get(17)?.parse().ok()?;
    let starttime: u64 = fields.get(19)?.parse().ok()?;

    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let mut uid = 0u32;
    let mut rss_kb = 0u64;
    for line in status.lines() {
        if let Some(value) = line.strip_prefix("Uid:") {
            uid = value.split_whitespace().next()?.parse().ok()?;
        } else if let Some(value) = line.strip_prefix("VmRSS:") {
            rss_kb = value.split_whitespace().next()?.parse().ok()?;
        }
    }

    let age_secs = starttime / clock_ticks.max(1.) as u64;
    let user = user_name(uid, users);
    Some(Raw {
        process: Process {
            pid,
            ppid,
            name,
            user,
            uid,
            state,
            cpu: 0.,
            rss_kb,
            threads,
            age_secs,
            history: Vec::new(),
        },
        ticks: utime + stime,
    })
}

/// Full command line, read lazily (the table never needs it).
#[allow(dead_code)]
pub fn command(pid: u32) -> String {
    let raw = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
    let mut cmd = String::from_utf8_lossy(&raw).replace('\0', " ");
    cmd.truncate(300);
    cmd.trim_end().to_string()
}

/// cgroup slices for a pid, used by the grouped view.
pub fn cgroups(pid: u32) -> Vec<String> {
    std::fs::read_to_string(format!("/proc/{pid}/cgroup"))
        .map(|raw| {
            raw.lines()
                .filter_map(|line| line.rsplit(':').next())
                .map(|path| path.trim().trim_start_matches('/').to_string())
                .filter(|path| !path.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

pub fn signal(pid: u32, sig: i32) -> Result<(), String> {
    let rc = unsafe { libc::kill(pid as i32, sig) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}

fn cpu_times() -> Option<(u64, u64)> {
    let stat = std::fs::read_to_string("/proc/stat").ok()?;
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

fn memory() -> (u64, u64) {
    let info = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let mut total = 0;
    let mut available = 0;
    for line in info.lines() {
        if let Some(value) = line.strip_prefix("MemTotal:") {
            total = value
                .split_whitespace()
                .next()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
        } else if let Some(value) = line.strip_prefix("MemAvailable:") {
            available = value
                .split_whitespace()
                .next()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
        }
    }
    (total, total.saturating_sub(available))
}

fn load_average() -> (f32, f32) {
    std::fs::read_to_string("/proc/loadavg")
        .ok()
        .and_then(|raw| {
            let fields: Vec<f32> = raw
                .split_whitespace()
                .take(2)
                .filter_map(|v| v.parse().ok())
                .collect();
            (fields.len() == 2).then(|| (fields[0], fields[1]))
        })
        .unwrap_or((0., 0.))
}

fn user_name(uid: u32, cache: &mut HashMap<u32, String>) -> String {
    if let Some(name) = cache.get(&uid) {
        return name.clone();
    }
    let mut buffer = vec![0i8; 1024];
    let mut passwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    let name = unsafe {
        let rc = libc::getpwuid_r(
            uid,
            &mut passwd,
            buffer.as_mut_ptr(),
            buffer.len(),
            &mut result,
        );
        if rc == 0 && !result.is_null() && !passwd.pw_name.is_null() {
            std::ffi::CStr::from_ptr(passwd.pw_name)
                .to_string_lossy()
                .into_owned()
        } else {
            uid.to_string()
        }
    };
    cache.insert(uid, name.clone());
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_the_live_machine() {
        let mut sampler = Sampler::new();
        let first = sampler.sample();
        assert!(!first.processes.is_empty());
        assert!(first.mem_total_kb > 0);
        let second = sampler.sample();
        assert!(second.processes.len() <= first.processes.len() + 32);
        // The sampler itself is always running.
        assert!(second.processes.iter().any(|p| p.pid == std::process::id()));
        assert!(second.processes.iter().all(|p| p.cpu >= 0.));
        assert!(second.threads >= 1);
    }

    #[test]
    fn state_labels_are_stable() {
        assert_eq!(State::from_letter("R").label(), "RUNNING");
        assert_eq!(State::from_letter("S").kind(), "sleep");
        assert_eq!(State::from_letter("D").kind(), "wait");
    }
}
