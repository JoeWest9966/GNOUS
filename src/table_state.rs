//! State for the `processes:table` view.
//!
//! Kept in one place so the sampling (which lives in `App`) and the rendering
//! (which lives in `pages/processes.rs`) never disagree about what the user is
//! looking at.

use crate::proc::Process;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

#[derive(Clone, Debug)]
pub struct TableState {
    pub sort: SortKey,
    pub descending: bool,
    pub scope: Scope,
    pub query: String,
    /// Freeze sampling while the user reads the numbers.
    pub paused: bool,
    /// Selection at the moment "排序冻结" was enabled: the row's index is
    /// remembered so it can be found again in the next frame.
    pub frozen_pid: Option<u32>,
}

impl Default for TableState {
    fn default() -> Self {
        Self {
            sort: SortKey::Cpu,
            descending: true,
            scope: Scope::All,
            query: String::new(),
            paused: false,
            frozen_pid: None,
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

    /// Filter + sort, sharing one snapshot with every other view.
    pub fn rows<'a>(&self, processes: &'a [Process], my_uid: u32) -> Vec<&'a Process> {
        let query = self.query.trim().to_lowercase();
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
            .collect();
        out.sort_by(|a, b| {
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
        // A frozen selection keeps its row position across refreshes.
        if let Some(pid) = self.frozen_pid {
            if let Some(index) = out.iter().position(|p| p.pid == pid) {
                let process = out.remove(index);
                out.insert(0, process);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proc::State as ProcState;

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
        // "s" still matches "systemd" (and not "firefox")
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
}
