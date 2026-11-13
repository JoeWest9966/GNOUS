//! `root-align`: a page-level diagnostic console over the built-in backend.
//!
//! `Action` / `Tab` / `State` are the external seam used by `App` and the
//! renderer. The backend returns structured facts; this module adapts them to
//! the existing UI types without parsing human stdout for health. `output`
//! remains the verbatim, human-readable result shown in the console.

use serde_json::Value;

pub use crate::fixes_backend;
use crate::sys;

pub const BUILTIN_STATE_FILE: &str = "~/.local/state/gnous/fixes/root-align.json";
pub use fixes_backend::root_align::Unit;
pub const UNITS: &[Unit] = fixes_backend::root_align::UNITS;
pub const DOMAINS: &[Domain] = &[
    Domain {
        name: "音频",
        description: "音量条、声音面板、系统提示音、录音 / 通话。",
    },
    Domain {
        name: "设备枚举 / 路由",
        description: "输出设备、音量路由、滤波器链。",
    },
    Domain {
        name: "媒体控制 / 蓝牙",
        description: "媒体键、GNOME 指示器、耳机播放控制。",
    },
    Domain {
        name: "全局搜索",
        description: "GNOME 概览检索本地文档内容。",
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Status,
    Doctor,
    Apply,
    Revert,
    RestartShell,
}

impl Action {
    pub const ALL: [Action; 5] = [
        Action::Status,
        Action::Doctor,
        Action::Apply,
        Action::Revert,
        Action::RestartShell,
    ];

    /// Stable operation names; tests assert that no shell flags can enter
    /// the backend through this interface.
    #[cfg(test)]
    pub fn arg(self) -> &'static str {
        match self {
            Action::Status => "status",
            Action::Doctor => "doctor",
            Action::Apply => "apply",
            Action::Revert => "revert",
            Action::RestartShell => "restart-shell",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Action::Status => "STATUS",
            Action::Doctor => "DOCTOR",
            Action::Apply => "APPLY",
            Action::Revert => "REVERT",
            Action::RestartShell => "RESTART SHELL",
        }
    }

    pub fn tab(self) -> Tab {
        match self {
            Action::Status => Tab::Status,
            Action::Doctor => Tab::Doctor,
            Action::Apply | Action::Revert | Action::RestartShell => Tab::Action,
        }
    }

    /// Every mutating command needs a second click before it runs.
    pub fn is_mutating(self) -> bool {
        matches!(self, Action::Apply | Action::Revert | Action::RestartShell)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Status,
    Doctor,
    Action,
}

impl Tab {
    pub fn label(self) -> &'static str {
        match self {
            Tab::Status => "STATUS",
            Tab::Doctor => "DOCTOR",
            Tab::Action => "APPLY LOG",
        }
    }
}

/// One upstream command result. `output` is stdout + stderr, never a summary.
#[derive(Clone, Debug)]
pub struct Outcome {
    pub action: Action,
    pub ok: bool,
    pub output: String,
    pub command: String,
    pub facts: Value,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Unknown,
    Aligned,
    Attention,
    Failed,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Aligned => "ALIGNED",
            Status::Attention => "ATTENTION",
            Status::Failed => "FAILED",
            Status::Unknown => "UNKNOWN",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitState {
    pub name: String,
    pub active: String,
    pub enabled: String,
    pub override_applied: bool,
    pub healthy: bool,
}

pub struct Domain {
    pub name: &'static str,
    pub description: &'static str,
}

#[derive(Clone, Debug, Default)]
pub struct State {
    pub status: Option<Outcome>,
    pub doctor: Option<Outcome>,
    pub action: Option<Outcome>,
    pub tab: Tab,
    pub domain: usize,
    pub busy: bool,
    /// First click on a mutating action only arms this; second click runs it.
    pub armed: Option<Action>,
    pub conclusion: Status,
    pub units: Vec<UnitState>,
    pub checked: String,
}

impl State {
    pub fn new() -> Self {
        Self {
            checked: sys::local_clock(),
            ..Default::default()
        }
    }

    pub fn outcome(&self, tab: Tab) -> Option<&Outcome> {
        match tab {
            Tab::Status => self.status.as_ref(),
            Tab::Doctor => self.doctor.as_ref(),
            Tab::Action => self.action.as_ref(),
        }
    }

    pub fn unit(&self, name: &str) -> Option<&UnitState> {
        self.units.iter().find(|unit| unit.name == name)
    }

    /// `switch_tab = false` keeps a visible APPLY LOG while the automatic
    /// status refresh arrives after apply/revert.
    pub fn absorb(&mut self, outcome: Outcome, switch_tab: bool) {
        let action = outcome.action;
        if switch_tab {
            self.tab = action.tab();
        }
        match action {
            Action::Status => {
                self.conclusion = status_from_facts(&outcome);
                self.units = units_from_facts(&outcome);
                self.checked = sys::local_clock();
                self.status = Some(outcome);
            }
            Action::Doctor => self.doctor = Some(outcome),
            Action::Apply | Action::Revert | Action::RestartShell => {
                self.action = Some(outcome);
            }
        }
        self.busy = false;
    }
}

fn status_from_facts(outcome: &Outcome) -> Status {
    if !outcome.ok {
        return Status::Failed;
    }
    match outcome.facts["status"].as_str().unwrap_or("unknown") {
        "aligned" => Status::Aligned,
        "attention" => Status::Attention,
        "failed" => Status::Failed,
        _ => Status::Unknown,
    }
}

fn units_from_facts(outcome: &Outcome) -> Vec<UnitState> {
    facts_units(&outcome.facts)
}

/// Convert backend facts into the renderer's row type. Unknown fields never
/// become healthy rows.
pub fn facts_units(facts: &Value) -> Vec<UnitState> {
    let Some(rows) = facts["units"].as_array() else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|row| {
            let name = row["name"].as_str()?.to_string();
            if !UNITS.iter().any(|unit| unit.name == name) {
                return None;
            }
            let active = row["active"].as_str().unwrap_or("unknown").to_string();
            let enabled = row["enabled"].as_str().unwrap_or("unknown").to_string();
            let override_applied = row["override_applied"] == true;
            let healthy = row["healthy"] == true;
            Some(UnitState {
                name,
                active,
                enabled,
                override_applied,
                healthy,
            })
        })
        .collect()
}

pub fn backend_op(action: Action) -> fixes_backend::Op {
    match action {
        Action::Status => fixes_backend::Op::Status,
        Action::Doctor => fixes_backend::Op::Doctor,
        Action::Apply => fixes_backend::Op::Apply,
        Action::Revert => fixes_backend::Op::Revert,
        Action::RestartShell => fixes_backend::Op::RestartShell,
    }
}

/// The one backend call. Called from a background executor, never on the UI
/// thread. Missing tools are impossible now; real failures still become
/// visible `FAILED` output.
pub fn run(action: Action) -> Outcome {
    let result = fixes_backend::run(fixes_backend::Tool::RootAlign, backend_op(action));
    Outcome {
        action,
        ok: result.ok,
        output: result.output,
        command: result.command,
        facts: result.facts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn outcome(facts: Value) -> Outcome {
        Outcome {
            action: Action::Status,
            ok: true,
            output: "stdout".into(),
            command: "gnous builtin root-align / status".into(),
            facts,
        }
    }

    #[test]
    fn real_facts_fill_all_eight_units() {
        let rows: Vec<_> = UNITS
            .iter()
            .map(|unit| json!({"name": unit.name, "active": "active", "enabled": "enabled", "override_applied": true, "healthy": true}))
            .collect();
        let mut state = State::default();
        state.absorb(outcome(json!({"status": "aligned", "units": rows})), true);
        assert_eq!(state.conclusion, Status::Aligned);
        assert_eq!(state.units.len(), 8);
        assert!(state.units.iter().all(|unit| unit.healthy));
    }

    #[test]
    fn unknown_facts_never_fake_health() {
        let mut state = State::default();
        state.absorb(outcome(json!({"status": "unknown"})), true);
        assert_eq!(state.conclusion, Status::Unknown);
        assert!(state.units.is_empty());
        let mut failed = outcome(json!({"status": "failed"}));
        failed.ok = false;
        state.absorb(failed, true);
        assert_eq!(state.conclusion, Status::Failed);
    }

    #[test]
    fn commands_are_built_in_and_have_no_dangerous_flags() {
        for action in Action::ALL {
            assert!(!action.arg().contains("--"));
            assert!(!action.arg().contains(".sh"));
            assert_eq!(
                backend_op(action),
                match action {
                    Action::Status => fixes_backend::Op::Status,
                    Action::Doctor => fixes_backend::Op::Doctor,
                    Action::Apply => fixes_backend::Op::Apply,
                    Action::Revert => fixes_backend::Op::Revert,
                    Action::RestartShell => fixes_backend::Op::RestartShell,
                }
            );
        }
        assert!(!Action::Status.is_mutating());
        assert!(!Action::Doctor.is_mutating());
        assert!(Action::Apply.is_mutating());
        assert!(Action::Revert.is_mutating());
        assert!(Action::RestartShell.is_mutating());
    }
}
