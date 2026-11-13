//! State for the `processes:table` view.
//!
//! Kept in one place so the sampling (which lives in `App`) and the rendering
//! (which lives in `pages/processes.rs`) never disagree about what the user is
//! looking at. Preferences are serialisable; transient UI state and the change
//! detector are explicitly skipped by `serde`.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::proc::{Process, State as ProcState};

/// The table renders at most this many rows, even when a filter matches more.
pub const ROW_LIMIT: usize = 120;
/// A process is "hot" when its CPU clears the chosen threshold *or* its
/// resident memory clears this fixed floor.
pub const HOT_MEM_KB: u64 = 512 * 1024;
/// How long a change marker stays on a row.
pub const HIGHLIGHT_TTL: Duration = Duration::from_millis(1600);

const CPU_CHANGE: f32 = 8.;
const MEM_CHANGE_KB: u64 = 64 * 1024;
const HOT_CPU_STEPS: [f32; 4] = [1., 5., 10., 20.];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortKey {
    Cpu,
    Name,
    Pid,
    State,
    Memory,
    Threads,
}

impl SortKey {
    pub fn label(self) -> &'static str {
        match self {
            SortKey::Cpu => "CPU",
            SortKey::Name => "名称",
            SortKey::Pid => "PID",
            SortKey::State => "状态",
            SortKey::Memory => "内存",
            SortKey::Threads => "线程",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    All,
    Mine,
    System,
}

impl Scope {
    pub fn label(self) -> &'static str {
        match self {
            Scope::All => "全部",
            Scope::Mine => "我的",
            Scope::System => "系统",
        }
    }
}

/// `4 · 状态过滤`. `All` is the neutral value; every other variant maps to
/// exactly one procfs state letter group.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateFilter {
    All,
    Running,
    Sleeping,
    Idle,
    IoWait,
    Stopped,
    Zombie,
    Other,
}

impl StateFilter {
    pub const ALL: [StateFilter; 8] = [
        StateFilter::All,
        StateFilter::Running,
        StateFilter::Sleeping,
        StateFilter::Idle,
        StateFilter::IoWait,
        StateFilter::Stopped,
        StateFilter::Zombie,
        StateFilter::Other,
    ];

    pub fn id(self) -> &'static str {
        match self {
            StateFilter::All => "state-all",
            StateFilter::Running => "state-running",
            StateFilter::Sleeping => "state-sleep",
            StateFilter::Idle => "state-idle",
            StateFilter::IoWait => "state-io",
            StateFilter::Stopped => "state-stopped",
            StateFilter::Zombie => "state-zombie",
            StateFilter::Other => "state-other",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            StateFilter::All => "全部",
            StateFilter::Running => "运行",
            StateFilter::Sleeping => "睡眠",
            StateFilter::Idle => "空闲",
            StateFilter::IoWait => "IO",
            StateFilter::Stopped => "停止",
            StateFilter::Zombie => "僵尸",
            StateFilter::Other => "其他",
        }
    }

    pub fn matches(self, state: ProcState) -> bool {
        match self {
            StateFilter::All => true,
            StateFilter::Running => state == ProcState::Running,
            StateFilter::Sleeping => state == ProcState::Sleeping,
            StateFilter::Idle => state == ProcState::Idle,
            StateFilter::IoWait => state == ProcState::IoWait,
            StateFilter::Stopped => state == ProcState::Stopped,
            StateFilter::Zombie => state == ProcState::Zombie,
            StateFilter::Other => state == ProcState::Other,
        }
    }

    pub fn next(self) -> Self {
        let index = Self::ALL.iter().position(|s| *s == self).unwrap_or(0);
        Self::ALL[(index + 1) % Self::ALL.len()]
    }
}

/// `30 · 行密度`. Padding changes, not the font: numbers stay legible in all
/// three modes and the column geometry never shifts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Density {
    Compact,
    Normal,
    Relaxed,
}

impl Density {
    pub fn label(self) -> &'static str {
        match self {
            Density::Compact => "紧凑",
            Density::Normal => "标准",
            Density::Relaxed => "宽松",
        }
    }

    pub fn padding(self) -> f32 {
        match self {
            Density::Compact => 5.5,
            Density::Normal => 11.,
            Density::Relaxed => 17.,
        }
    }

    pub fn next(self) -> Self {
        match self {
            Density::Compact => Density::Normal,
            Density::Normal => Density::Relaxed,
            Density::Relaxed => Density::Compact,
        }
    }
}

/// `20 · 刷新频率`. The sampler polls four times a second and only reads
/// `/proc` when the selected interval has actually elapsed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshRate {
    Fast,
    Normal,
    Slow,
    Lazy,
}

impl RefreshRate {
    pub fn label(self) -> &'static str {
        match self {
            RefreshRate::Fast => "0.5s",
            RefreshRate::Normal => "1s",
            RefreshRate::Slow => "2s",
            RefreshRate::Lazy => "5s",
        }
    }

    pub fn duration(self) -> Duration {
        match self {
            RefreshRate::Fast => Duration::from_millis(500),
            RefreshRate::Normal => Duration::from_secs(1),
            RefreshRate::Slow => Duration::from_secs(2),
            RefreshRate::Lazy => Duration::from_secs(5),
        }
    }

    pub fn next(self) -> Self {
        match self {
            RefreshRate::Fast => RefreshRate::Normal,
            RefreshRate::Normal => RefreshRate::Slow,
            RefreshRate::Slow => RefreshRate::Lazy,
            RefreshRate::Lazy => RefreshRate::Fast,
        }
    }
}

/// A persistent watch rule. PIDs are recycled across reboots, so a watch is
/// keyed by the process name and its owner instead. Both fields come from the
/// live snapshot at the moment the user pressed the star.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Watch {
    pub name: String,
    pub user: String,
    pub uid: u32,
}

impl Watch {
    fn from_process(process: &Process) -> Self {
        Self {
            name: process.name.clone(),
            user: process.user.clone(),
            uid: process.uid,
        }
    }

    fn matches(&self, process: &Process) -> bool {
        self.name == process.name && self.user == process.user && self.uid == process.uid
    }
}

/// `21 · 变化高亮`. The marker is derived from two real samples, never from a
/// timer alone: new row, state transition, CPU jump or RSS jump.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    New,
    State,
    Cpu,
    Memory,
}

impl ChangeKind {
    pub fn label(self) -> &'static str {
        match self {
            ChangeKind::New => "NEW",
            ChangeKind::State => "STATE",
            ChangeKind::Cpu => "CPU",
            ChangeKind::Memory => "MEM",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Highlight {
    kind: ChangeKind,
    at: Instant,
}

#[derive(Clone, Copy, Debug)]
struct Metrics {
    state: ProcState,
    cpu: f32,
    rss_kb: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TableState {
    pub sort: SortKey,
    pub descending: bool,
    pub scope: Scope,
    pub state_filter: StateFilter,
    pub hot_only: bool,
    pub hot_cpu: f32,
    pub watched_only: bool,
    pub watched: Vec<Watch>,
    pub refresh: RefreshRate,
    pub density: Density,
    pub highlights_on: bool,

    #[serde(skip)]
    pub query: String,
    /// Freeze sampling while the user reads the numbers.
    #[serde(skip)]
    pub paused: bool,
    /// Selection at the moment "排序冻结" was enabled: the row's index is
    /// remembered so it can be found again in the next frame.
    #[serde(skip)]
    pub frozen_pid: Option<u32>,
    #[serde(skip)]
    highlights: HashMap<u32, Highlight>,
    #[serde(skip)]
    last: HashMap<u32, Metrics>,
}

impl Default for TableState {
    fn default() -> Self {
        Self {
            sort: SortKey::Cpu,
            descending: true,
            scope: Scope::All,
            state_filter: StateFilter::All,
            hot_only: false,
            hot_cpu: 5.,
            watched_only: false,
            watched: Vec::new(),
            refresh: RefreshRate::Normal,
            density: Density::Normal,
            highlights_on: true,
            query: String::new(),
            paused: false,
            frozen_pid: None,
            highlights: HashMap::new(),
            last: HashMap::new(),
        }
    }
}

impl TableState {
    pub fn sort_by(&mut self, key: SortKey) {
        if self.sort == key {
            self.descending = !self.descending;
        } else {
            self.sort = key;
            self.descending = true;
        }
    }

    pub fn set_query_char(&mut self, ch: char) {
        self.query.push(ch);
    }

    pub fn backspace(&mut self) {
        self.query.pop();
    }

    pub fn clear_query(&mut self) {
        self.query.clear();
    }

    pub fn hot_threshold(&self) -> f32 {
        self.hot_cpu.clamp(0.5, 90.)
    }

    pub fn cycle_hot_cpu(&mut self) {
        let index = HOT_CPU_STEPS
            .iter()
            .position(|step| (*step - self.hot_cpu).abs() < 0.01)
            .unwrap_or(1);
        self.hot_cpu = HOT_CPU_STEPS[(index + 1) % HOT_CPU_STEPS.len()];
    }

    pub fn set_state_filter(&mut self, filter: StateFilter) {
        self.state_filter = filter;
    }

    pub fn cycle_state_filter(&mut self) {
        self.state_filter = self.state_filter.next();
    }

    pub fn toggle_hot_only(&mut self) {
        self.hot_only = !self.hot_only;
    }

    pub fn toggle_watched_only(&mut self) {
        self.watched_only = !self.watched_only;
    }

    pub fn toggle_highlights(&mut self) {
        self.highlights_on = !self.highlights_on;
        if !self.highlights_on {
            self.highlights.clear();
        }
    }

    pub fn cycle_refresh(&mut self) {
        self.refresh = self.refresh.next();
    }

    pub fn cycle_density(&mut self) {
        self.density = self.density.next();
    }

    pub fn is_watched(&self, process: &Process) -> bool {
        self.watched.iter().any(|watch| watch.matches(process))
    }

    pub fn watch_count(&self) -> usize {
        self.watched.len()
    }

    /// Returns whether the process is watched *after* the toggle.
    pub fn toggle_watch(&mut self, process: &Process) -> bool {
        if let Some(index) = self.watched.iter().position(|watch| watch.matches(process)) {
            self.watched.remove(index);
            false
        } else {
            self.watched.push(Watch::from_process(process));
            true
        }
    }

    fn pin_rank(&self, process: &Process) -> u8 {
        if self.frozen_pid == Some(process.pid) {
            0
        } else if self.is_watched(process) {
            1
        } else {
            2
        }
    }

    /// Filter + sort, sharing one snapshot with every other view. Frozen and
    /// watched rows pin to the top; the chosen sort still applies *within*
    /// each group, so the table never lies about its order.
    pub fn rows<'a>(&self, processes: &'a [Process], my_uid: u32) -> Vec<&'a Process> {
        let query = self.query.trim().to_lowercase();
        let hot_cpu = self.hot_threshold();
        let mut out: Vec<&Process> = processes
            .iter()
            .filter(|p| match self.scope {
                Scope::All => true,
                Scope::Mine => p.uid == my_uid,
                Scope::System => p.uid != my_uid,
            })
            .filter(|p| {
                query.is_empty()
                    || p.name.to_lowercase().contains(&query)
                    || p.pid.to_string().contains(&query)
                    || p.user.to_lowercase().contains(&query)
            })
            .filter(|p| self.state_filter.matches(p.state))
            .filter(|p| !self.hot_only || p.cpu >= hot_cpu || p.rss_kb >= HOT_MEM_KB)
            .filter(|p| !self.watched_only || self.is_watched(p))
            .collect();
        out.sort_by(|a, b| {
            let rank = self.pin_rank(a).cmp(&self.pin_rank(b));
            if rank != std::cmp::Ordering::Equal {
                return rank;
            }
            let order = match self.sort {
                SortKey::Cpu => a.cpu.total_cmp(&b.cpu),
                SortKey::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                SortKey::Pid => a.pid.cmp(&b.pid),
                SortKey::State => a.state.label().cmp(b.state.label()),
                SortKey::Memory => a.rss_kb.cmp(&b.rss_kb),
                SortKey::Threads => a.threads.cmp(&b.threads),
            };
            if self.descending {
                order.reverse()
            } else {
                order
            }
        });
        out
    }

    /// PIDs in the exact order the table renders them, for keyboard selection
    /// and `ScrollHandle::scroll_to_item`.
    pub fn visible_pids(&self, processes: &[Process], my_uid: u32) -> Vec<u32> {
        self.rows(processes, my_uid)
            .into_iter()
            .take(ROW_LIMIT)
            .map(|p| p.pid)
            .collect()
    }

    /// Seed the change detector from the first sample: opening the page must
    /// not paint every process as new.
    pub fn prime(&mut self, processes: &[Process]) {
        self.last = metrics(processes);
        self.highlights.clear();
    }

    /// Compare the newest sample with the previous one and mark honest deltas.
    pub fn note_changes(&mut self, processes: &[Process]) {
        let current = metrics(processes);
        let live: HashSet<u32> = current.keys().copied().collect();
        self.highlights.retain(|pid, _| live.contains(pid));
        if self.highlights_on {
            let now = Instant::now();
            for process in processes {
                let kind = match self.last.get(&process.pid) {
                    None => Some(ChangeKind::New),
                    Some(prev) if prev.state != process.state => Some(ChangeKind::State),
                    Some(prev) if (process.cpu - prev.cpu).abs() >= CPU_CHANGE => {
                        Some(ChangeKind::Cpu)
                    }
                    Some(prev) if process.rss_kb.abs_diff(prev.rss_kb) >= MEM_CHANGE_KB => {
                        Some(ChangeKind::Memory)
                    }
                    _ => None,
                };
                if let Some(kind) = kind {
                    self.highlights
                        .insert(process.pid, Highlight { kind, at: now });
                }
            }
        }
        self.last = current;
    }

    pub fn highlight_kind(&self, pid: u32) -> Option<ChangeKind> {
        if !self.highlights_on {
            return None;
        }
        self.highlights
            .get(&pid)
            .filter(|highlight| highlight.at.elapsed() < HIGHLIGHT_TTL)
            .map(|highlight| highlight.kind)
    }

    /// Returns whether any marker expired (so the caller can re-render).
    pub fn expire_highlights(&mut self) -> bool {
        let before = self.highlights.len();
        self.highlights
            .retain(|_, highlight| highlight.at.elapsed() < HIGHLIGHT_TTL);
        before != self.highlights.len()
    }
}

fn metrics(processes: &[Process]) -> HashMap<u32, Metrics> {
    processes
        .iter()
        .map(|p| {
            (
                p.pid,
                Metrics {
                    state: p.state,
                    cpu: p.cpu,
                    rss_kb: p.rss_kb,
                },
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(pid: u32, uid: u32, name: &str, cpu: f32, mem: u64) -> Process {
        Process {
            pid,
            ppid: 1,
            name: name.into(),
            user: if uid == 0 { "root".into() } else { "me".into() },
            uid,
            state: ProcState::Sleeping,
            cpu,
            rss_kb: mem,
            threads: 1,
            age_secs: 0,
            history: Vec::new(),
        }
    }

    #[test]
    fn sorts_by_key_and_reverses() {
        let procs = vec![
            process(1, 1000, "alpha", 2., 100),
            process(2, 1000, "beta", 8., 900),
        ];
        let mut state = TableState::default();
        assert_eq!(state.rows(&procs, 1000)[0].name, "beta");
        state.sort_by(SortKey::Cpu);
        assert_eq!(state.rows(&procs, 1000)[0].name, "alpha");
        state.sort_by(SortKey::Name);
        assert!(state.descending);
    }

    #[test]
    fn filters_by_scope_and_query() {
        let procs = vec![
            process(10, 1000, "firefox", 1., 100),
            process(11, 0, "systemd", 0., 100),
        ];
        let mut state = TableState::default();
        state.scope = Scope::Mine;
        assert_eq!(state.rows(&procs, 1000).len(), 1);
        state.scope = Scope::All;
        state.set_query_char('s');
        state.set_query_char('y');
        assert_eq!(state.rows(&procs, 1000)[0].name, "systemd");
        state.backspace();
        assert_eq!(state.rows(&procs, 1000).len(), 1);
        state.clear_query();
        assert_eq!(state.rows(&procs, 1000).len(), 2);
    }

    #[test]
    fn frozen_selection_stays_on_top() {
        let procs = vec![
            process(1, 1000, "a", 9., 100),
            process(2, 1000, "b", 1., 100),
        ];
        let mut state = TableState::default();
        state.frozen_pid = Some(2);
        assert_eq!(state.rows(&procs, 1000)[0].pid, 2);
    }

    #[test]
    fn state_and_hot_filters_are_independent() {
        let mut running = process(1, 1000, "busy", 12., 100);
        running.state = ProcState::Running;
        let sleepy = process(2, 1000, "cache", 0., 600 * 1024);
        let quiet = process(3, 1000, "quiet", 0., 100);
        let procs = vec![running, sleepy, quiet];
        let mut state = TableState::default();
        state.state_filter = StateFilter::Running;
        assert_eq!(state.rows(&procs, 1000).len(), 1);
        state.state_filter = StateFilter::All;
        state.hot_only = true;
        // CPU or RSS: both the busy process and the big cache count.
        assert_eq!(state.rows(&procs, 1000).len(), 2);
        state.cycle_hot_cpu();
        assert_eq!(state.hot_threshold(), 10.);
        assert_eq!(state.rows(&procs, 1000).len(), 2);
    }

    #[test]
    fn watched_rows_pin_and_can_be_the_only_scope() {
        let procs = vec![
            process(1, 1000, "a", 9., 100),
            process(2, 1000, "b", 1., 100),
            process(3, 1000, "c", 20., 100),
        ];
        let mut state = TableState::default();
        assert!(state.toggle_watch(&procs[1]));
        assert!(state.toggle_watch(&procs[2]));
        assert!(state.is_watched(&procs[1]));
        let rows = state.rows(&procs, 1000);
        // Watched rows come first, still sorted by CPU within the group.
        assert_eq!(
            rows.iter().map(|p| p.pid).collect::<Vec<_>>(),
            vec![3, 2, 1]
        );
        state.watched_only = true;
        assert_eq!(state.rows(&procs, 1000).len(), 2);
        assert!(!state.toggle_watch(&procs[1]));
        assert_eq!(state.rows(&procs, 1000).len(), 1);
        state.watched_only = false;
        state.frozen_pid = Some(1);
        assert_eq!(state.rows(&procs, 1000)[0].pid, 1);
    }

    #[test]
    fn change_markers_come_from_two_real_samples() {
        let mut procs = vec![
            process(1, 1000, "a", 1., 100),
            process(2, 1000, "b", 0., 100),
        ];
        let mut state = TableState::default();
        state.prime(&procs);
        assert_eq!(state.highlight_kind(1), None);
        procs[0].state = ProcState::Running;
        procs[0].cpu = 30.;
        state.note_changes(&procs);
        assert_eq!(state.highlight_kind(1), Some(ChangeKind::State));
        assert_eq!(state.highlight_kind(2), None);
        for highlight in state.highlights.values_mut() {
            highlight.at = Instant::now() - HIGHLIGHT_TTL - Duration::from_millis(1);
        }
        assert!(state.expire_highlights());
        assert_eq!(state.highlight_kind(1), None);
    }

    #[test]
    fn visible_pids_are_capped() {
        let procs: Vec<Process> = (0..ROW_LIMIT + 5)
            .map(|index| process(index as u32 + 1, 1000, "p", 0., 100))
            .collect();
        let state = TableState::default();
        assert_eq!(state.visible_pids(&procs, 1000).len(), ROW_LIMIT);
    }
}
