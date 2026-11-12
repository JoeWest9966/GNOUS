//! The right-column switches, backed by whatever this machine actually exposes.
//!
//! Each switch is a thin spec: a gsettings boolean, a gsettings choice (e.g.
//! `color-scheme = prefer-dark`), or an external command (power profiles).
//! Missing schemas/commands simply make the switch unavailable instead of
//! showing a fake value, and there is no caching: [`Toggles::read`] is the
//! source of truth, called on startup, on every toggle, and on a slow tick.

use std::process::Command;

pub struct Toggle {
    pub id: &'static str,
    pub title: &'static str,
    /// What it maps to (`color-scheme → prefer-dark`), or why it is missing.
    pub description: String,
    pub on: bool,
    pub available: bool,
}

/// Stateless facade: the specs are constants, state lives in gsettings.
#[derive(Default)]
pub struct Toggles;

impl Toggles {
    pub fn new() -> Self {
        Self
    }

    pub fn read(&self) -> Vec<Toggle> {
        SPECS.iter().map(probe).collect()
    }

    pub fn set(&self, id: &str, on: bool) -> Result<(), String> {
        let spec = SPECS
            .iter()
            .find(|spec| spec.id == id)
            .ok_or_else(|| format!("未知开关: {id}"))?;
        spec.kind.write(on)
    }
}

struct Spec {
    id: &'static str,
    title: &'static str,
    description: &'static str,
    /// Shown instead of `description` when the backend is missing.
    hint: &'static str,
    kind: Kind,
}

enum Kind {
    /// gsettings boolean; `inverted` flips the UI meaning (show-banners).
    Bool {
        schema: &'static str,
        key: &'static str,
        inverted: bool,
    },
    /// gsettings string that equals `on_value` when the switch is on.
    Choice {
        schema: &'static str,
        key: &'static str,
        on_value: &'static str,
    },
    /// External command whose probe output equals `on_value` when on.
    Process {
        probe: &'static [&'static str],
        on: &'static [&'static str],
        off: &'static [&'static str],
        on_value: &'static str,
    },
}

impl Kind {
    fn read(&self) -> Option<bool> {
        match self {
            Kind::Bool {
                schema,
                key,
                inverted,
            } => gsettings_get(schema, key).map(|raw| bool_state(&raw, *inverted)),
            Kind::Choice {
                schema,
                key,
                on_value,
            } => gsettings_get(schema, key).map(|raw| choice_state(&raw, on_value)),
            Kind::Process {
                probe, on_value, ..
            } => run(probe).map(|raw| process_state(&raw, on_value)),
        }
    }

    fn write(&self, on: bool) -> Result<(), String> {
        match self {
            Kind::Bool {
                schema,
                key,
                inverted,
            } => {
                let value = if on != *inverted { "true" } else { "false" };
                gsettings_set(schema, key, value)
            }
            Kind::Choice {
                schema,
                key,
                on_value,
            } => {
                let value = if on { *on_value } else { "default" };
                gsettings_set(schema, key, value)
            }
            Kind::Process {
                on: on_cmd, off, ..
            } => run(if on { on_cmd } else { off })
                .map(|_| ())
                .ok_or_else(|| "命令执行失败".to_string()),
        }
    }
}

const SPECS: &[Spec] = &[
    Spec {
        id: "dark",
        title: "深色界面",
        description: "color-scheme → prefer-dark",
        hint: "未找到 GNOME 桌面设置",
        kind: Kind::Choice {
            schema: "org.gnome.desktop.interface",
            key: "color-scheme",
            on_value: "prefer-dark",
        },
    },
    Spec {
        id: "dnd",
        title: "勿扰模式",
        description: "通知横幅已关闭",
        hint: "未找到通知设置",
        kind: Kind::Bool {
            schema: "org.gnome.desktop.notifications",
            key: "show-banners",
            inverted: true,
        },
    },
    Spec {
        id: "performance",
        title: "性能模式",
        description: "power-profiles-daemon → performance",
        hint: "未安装 power-profiles-daemon",
        kind: Kind::Process {
            probe: &["powerprofilesctl", "get"],
            on: &["powerprofilesctl", "set", "performance"],
            off: &["powerprofilesctl", "set", "balanced"],
            on_value: "performance",
        },
    },
    Spec {
        id: "night-light",
        title: "夜间色温",
        description: "night-light-enabled",
        hint: "未找到色彩插件设置",
        kind: Kind::Bool {
            schema: "org.gnome.settings-daemon.plugins.color",
            key: "night-light-enabled",
            inverted: false,
        },
    },
    Spec {
        id: "tiling",
        title: "自动平铺",
        description: "Pop Shell · tile-by-default",
        hint: "未安装 Pop Shell",
        kind: Kind::Bool {
            schema: "org.gnome.shell.extensions.pop-shell",
            key: "tile-by-default",
            inverted: false,
        },
    },
];

fn probe(spec: &Spec) -> Toggle {
    let state = spec.kind.read();
    Toggle {
        id: spec.id,
        title: spec.title,
        description: match state {
            Some(_) => spec.description.to_string(),
            None => spec.hint.to_string(),
        },
        on: state.unwrap_or(false),
        available: state.is_some(),
    }
}

// ============================================================================
// value helpers (pure, unit-tested)
// ============================================================================

fn bool_state(raw: &str, inverted: bool) -> bool {
    (raw.trim() == "true") != inverted
}

fn choice_state(raw: &str, on_value: &str) -> bool {
    raw.trim().trim_matches('\'') == on_value
}

fn process_state(raw: &str, on_value: &str) -> bool {
    raw.trim() == on_value
}

// ============================================================================
// process plumbing
// ============================================================================

fn run(args: &[&str]) -> Option<String> {
    let (program, rest) = args.split_first()?;
    let out = Command::new(program).args(rest).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn gsettings_get(schema: &str, key: &str) -> Option<String> {
    run(&["gsettings", "get", schema, key])
}

fn gsettings_set(schema: &str, key: &str, value: &str) -> Result<(), String> {
    let out = Command::new("gsettings")
        .args(["set", schema, key, value])
        .output()
        .map_err(|error| format!("无法执行 gsettings: {error}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_booleans_with_inversion() {
        assert!(bool_state("true", false));
        assert!(!bool_state("false", false));
        // show-banners=false means Do Not Disturb is on
        assert!(bool_state("false", true));
        assert!(!bool_state("true", true));
    }

    #[test]
    fn reads_choices_and_processes() {
        assert!(choice_state("'prefer-dark'\n", "prefer-dark"));
        assert!(!choice_state("'default'", "prefer-dark"));
        assert!(process_state("performance\n", "performance"));
        assert!(!process_state("balanced\n", "performance"));
    }

    #[test]
    fn every_spec_probes_and_reports() {
        let toggles = Toggles::new();
        let read = toggles.read();
        assert_eq!(read.len(), SPECS.len());
        for toggle in read {
            assert!(!toggle.title.is_empty());
            assert!(!toggle.description.is_empty(), "{}", toggle.id);
            if !toggle.available {
                assert!(!toggle.on, "{} 不可用却有状态", toggle.id);
            }
        }
        assert!(toggles.set("nope", true).is_err());
    }
}
