//! `root-align`: a page-level extension around `Root用户桌面功能对齐`.
//!
//! The UI is an honest console: `status` / `doctor` / `apply` / `revert` are
//! real invocations of the upstream `root-gnome-align.sh`, and every stdout is
//! kept verbatim. Nothing is inferred when the tool is missing or its output
//! cannot be parsed. `restart-shell` is a separate, two-step action.
//!
//! Manifest: `extensions/root-align/manifest.toml`.

use std::path::PathBuf;
use std::process::Command;

use crate::{fixes, sys};

pub const TOOL_DIR: &str = "Root用户桌面功能对齐";
pub const SCRIPT: &str = "root-gnome-align.sh";
pub const STATE_FILE: &str = "/root/.config/root-gnome-align/state.json";

/// The toolkit shares `GNOUS_FIXES_ROOT` with `gnome-fixes`; both live under
/// the same `/home/root/GNOME` directory by default.
pub fn tool_dir() -> PathBuf {
    fixes::root().join(TOOL_DIR)
}

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

pub struct Unit {
    pub name: &'static str,
    pub description: &'static str,
    pub domain: usize,
}

pub struct Domain {
    pub name: &'static str,
    pub description: &'static str,
}

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

/// The same eight units `root-gnome-align.sh status` reports, grouped by the
/// functional domains shown in the left column of the console.
pub const UNITS: &[Unit] = &[
    Unit {
        name: "pipewire.service",
        description: "PipeWire 媒体核心",
        domain: 0,
    },
    Unit {
        name: "pipewire.socket",
        description: "控制 / 管理套接字",
        domain: 0,
    },
    Unit {
        name: "pipewire-pulse.service",
        description: "PulseAudio 兼容层",
        domain: 0,
    },
    Unit {
        name: "pipewire-pulse.socket",
        description: "PulseAudio 兼容套接字",
        domain: 0,
    },
    Unit {
        name: "wireplumber.service",
        description: "设备枚举 / 音量路由",
        domain: 1,
    },
    Unit {
        name: "filter-chain.service",
        description: "PipeWire 滤波器链",
        domain: 1,
    },
    Unit {
        name: "mpris-proxy.service",
        description: "蓝牙 MPRIS 媒体代理",
        domain: 2,
    },
    Unit {
        name: "localsearch-3.service",
        description: "Tracker 文件索引",
        domain: 3,
    },
];

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

    /// `switch_tab = false` is used by the automatic status recheck after an
    /// apply/revert: the action log stays visible while fresh state arrives.
    pub fn absorb(&mut self, outcome: Outcome, switch_tab: bool) {
        let action = outcome.action;
        if switch_tab {
            self.tab = action.tab();
        }
        match action {
            Action::Status => {
                self.conclusion = conclusion(outcome.ok, &outcome.output);
                self.units = parse_units(&outcome.output);
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

pub fn conclusion(ok: bool, output: &str) -> Status {
    if !ok {
        Status::Failed
    } else if output.contains("已完全对齐") {
        Status::Aligned
    } else if output.contains("未完全对齐") {
        Status::Attention
    } else {
        Status::Unknown
    }
}

/// Parse the eight `root-gnome-align.sh status` rows. Unknown lines are
/// skipped, so a future tool version cannot produce imaginary green units.
pub fn parse_units(output: &str) -> Vec<UnitState> {
    let mut units = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        let mut parts = trimmed.split_whitespace();
        let first = parts.next().unwrap_or_default();
        let name = if matches!(first, "✓" | "!" | "✗") {
            parts.next().unwrap_or_default()
        } else {
            first
        };
        if !UNITS.iter().any(|unit| unit.name == name) {
            continue;
        }
        let mut active = "unknown".to_string();
        let mut enabled = "unknown".to_string();
        let mut override_applied = false;
        for token in parts {
            if let Some(value) = token.strip_prefix("active=") {
                active = value.to_string();
            } else if let Some(value) = token.strip_prefix("enabled=") {
                enabled = value.to_string();
            } else if let Some(value) = token.strip_prefix("覆盖=") {
                override_applied = value == "true";
            }
        }
        let healthy = active == "active" && override_applied;
        units.push(UnitState {
            name: name.to_string(),
            active,
            enabled,
            override_applied,
            healthy,
        });
    }
    units
}

/// The launcher is invoked through `bash`, so a fresh checkout without the
/// executable bit still works. The script resolves its own binary via `HERE`.
pub fn command_line(action: Action) -> (PathBuf, Vec<String>) {
    let script = tool_dir().join(SCRIPT);
    (
        PathBuf::from("bash"),
        vec![
            script.to_string_lossy().into_owned(),
            action.arg().to_string(),
        ],
    )
}

/// Run one upstream action. Called from a background executor, never on the UI
/// thread. Missing tools and launch failures become visible `FAILED` output.
pub fn run(action: Action) -> Outcome {
    let dir = tool_dir();
    let script = dir.join(SCRIPT);
    let (exe, args) = command_line(action);
    let command = format!("./{SCRIPT} {}", action.arg());
    if !script.exists() {
        return Outcome {
            action,
            ok: false,
            output: format!(
                "工具未安装：{}\n\n设置 {} 指向工具目录。",
                script.display(),
                fixes::ROOT_ENV
            ),
            command,
        };
    }
    let result = Command::new(&exe).args(&args).current_dir(&dir).output();
    let (ok, output) = match result {
        Ok(out) => {
            let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
            let err = String::from_utf8_lossy(&out.stderr);
            if !err.is_empty() {
                text.push_str("\n--- stderr ---\n");
                text.push_str(&err);
            }
            if text.trim().is_empty() {
                text = format!("{command} 已执行（无输出）");
            }
            (out.status.success(), text)
        }
        Err(error) => (false, format!("启动 {command} 失败：{error}")),
    };
    Outcome {
        action,
        ok,
        output,
        command,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "── 当前对齐状态 ──\n  ✓ pipewire.service           active=active     enabled=enabled    覆盖=true  PipeWire 媒体核心（音频路由）\n  ✓ pipewire.socket            active=active     enabled=enabled    覆盖=true  PipeWire 控制/管理套接字\n  ✓ pipewire-pulse.service     active=active     enabled=enabled    覆盖=true  PulseAudio 兼容层\n  ✓ pipewire-pulse.socket      active=active     enabled=enabled    覆盖=true  PulseAudio 兼容套接字\n  ✓ wireplumber.service        active=active     enabled=enabled    覆盖=true  会话媒体管理器\n  ✓ filter-chain.service       active=active     enabled=enabled    覆盖=true  PipeWire 滤波器链\n  ✓ mpris-proxy.service        active=active     enabled=enabled    覆盖=true  蓝牙 MPRIS 媒体代理\n  ✓ localsearch-3.service      active=active     enabled=enabled    覆盖=true  Tracker 文件索引\n\n状态：已完全对齐 ✅";

    #[test]
    fn parses_all_real_status_rows() {
        let units = parse_units(SAMPLE);
        assert_eq!(units.len(), UNITS.len());
        assert!(units.iter().all(|unit| unit.healthy));
        assert_eq!(units[0].active, "active");
        assert_eq!(units[0].enabled, "enabled");
        assert_eq!(conclusion(true, SAMPLE), Status::Aligned);
        assert_eq!(conclusion(true, "状态：未完全对齐"), Status::Attention);
        assert_eq!(conclusion(false, "工具未安装"), Status::Failed);
        assert_eq!(conclusion(true, ""), Status::Unknown);
    }

    #[test]
    fn unknown_output_never_fakes_health() {
        assert!(parse_units("随便一行输出").is_empty());
        let mut state = State::default();
        let outcome = Outcome {
            action: Action::Status,
            ok: true,
            output: "无法识别".into(),
            command: "status".into(),
        };
        state.absorb(outcome, true);
        assert_eq!(state.conclusion, Status::Unknown);
        assert!(state.units.is_empty());
    }

    #[test]
    fn commands_are_read_only_or_explicit_and_have_no_dangerous_flags() {
        for action in Action::ALL {
            assert!(!action.arg().contains("--"));
            let (_, args) = command_line(action);
            assert!(args[0].ends_with(SCRIPT));
            assert_eq!(args[1], action.arg());
            assert_eq!(args.len(), 2);
        }
        assert!(!Action::Status.is_mutating());
        assert!(!Action::Doctor.is_mutating());
        assert!(Action::Apply.is_mutating());
        assert!(Action::Revert.is_mutating());
        assert!(Action::RestartShell.is_mutating());
    }
}
