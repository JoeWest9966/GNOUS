//! Compile-time and run-time safety gates for the proxy-tools backend.
//!
//! The proxy extension manages the very relay that carries this machine's
//! outbound traffic. A wrong `systemctl --user` verb there does not just fail a
//! test — it takes the whole session offline, and any agent driving it loses
//! the connection it would need to fix the mistake. So the dangerous verbs are
//! not "discouraged by convention": they are unreachable here.
//!
//! Rules encoded below (kept in sync with `extensions/proxy-tools/DEV-SAFETY.md`):
//!
//! 1. The legacy relay unit is read-only. No command built by this module may
//!    name it in a mutating position.
//! 2. Only our own unit may be started/stopped/enabled/disabled.
//! 3. `set-environment` / `unset-environment` / `daemon-reload` / `kill` /
//!    `restart` never appear at all.
//! 4. `gsettings set|reset org.gnome.system.proxy` is never executed by GNOUS;
//!    control-plane changes are emitted as copyable commands for the operator.
//! 5. Protected ports (the live relay and unrelated local services) may never
//!    be bound or probed for changes.

use std::fmt;

/// Units owned by somebody else. Read-only for GNOUS.
pub const LEGACY_UNITS: &[&str] = &["j1900-proxy.service", "j1900-proxy"];

/// The only unit this backend is allowed to mutate.
pub const OWN_UNIT: &str = "gnous-proxy-relay.service";

/// Ports that carry the machine's live connectivity (or unrelated services).
/// The backend may *read* whether the configured listen port is occupied, but
/// it may never bind these unless they are the configured port of our own unit.
pub const PROTECTED_PORTS: &[u16] = &[31001, 17670, 8089, 8765, 8770]; // safety-allow: 常量定义，就是用来拒绝这些端口的

/// systemd verbs that can interrupt an in-flight connection.
const FORBIDDEN_VERBS: &[&str] = &[
    "set-environment",
    "unset-environment",
    "daemon-reload",
    "reload",
    "restart",
    "kill",
    "try-restart",
    "reload-or-restart",
    "isolate",
    "mask",
    "unmask",
    "reset-failed",
];

/// Verbs allowed against [`OWN_UNIT`] only.
const OWN_UNIT_VERBS: &[&str] = &["start", "stop", "enable", "disable"];

/// Read-only systemd verbs, allowed for any unit name.
const READONLY_VERBS: &[&str] = &[
    "show",
    "cat",
    "is-active",
    "is-enabled",
    "is-failed",
    "status",
    "list-units",
    "show-environment",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SafetyError {
    ForbiddenVerb(String),
    ForeignUnit(String),
    ProtectedPort(u16),
    ForeignProgram(String),
    NotAllowlisted(String),
}

impl fmt::Display for SafetyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SafetyError::ForbiddenVerb(verb) => {
                write!(
                    f,
                    "被安全规则拒绝的 systemd 动词：{verb}（会中断承载本机联网的中继）"
                )
            }
            SafetyError::ForeignUnit(unit) => {
                write!(
                    f,
                    "不允许改动的外部单元：{unit}（GNOUS 只读它，只启停自己的单元）"
                )
            }
            SafetyError::ProtectedPort(port) => write!(
                f,
                "端口 {port} 受保护（要么是本机联网中继，要么与代理无关），不得绑定或改动"
            ),
            SafetyError::ForeignProgram(program) => {
                write!(f, "不在允许清单内的可执行程序：{program}")
            }
            SafetyError::NotAllowlisted(joined) => {
                write!(f, "命令不在静态允许清单内：{joined}")
            }
        }
    }
}

impl std::error::Error for SafetyError {}

/// Programs this backend may execute. Everything is read-only or writes to
/// GNOUS-owned paths; nothing speaks to the running relay.
pub const ALLOWED_PROGRAMS: &[&str] = &["systemctl", "ip", "sh", "gsettings"];

pub fn check_program(program: &str) -> Result<(), SafetyError> {
    if ALLOWED_PROGRAMS.contains(&program) {
        return Ok(());
    }
    Err(SafetyError::ForeignProgram(program.to_string()))
}

/// Opt-in escape hatch used by the *installed* unit after a deliberate
/// handover. It is never set while developing this extension.
pub const ALLOW_PROTECTED_ENV: &str = "GNOUS_PROXY_ALLOW_PROTECTED_PORT";

fn protected_override() -> bool {
    matches!(
        std::env::var(ALLOW_PROTECTED_ENV).as_deref(),
        Ok("1") | Ok("true")
    )
}

/// Pure rule, testable without touching the process environment.
pub fn check_port_with(port: u16, binding: bool, override_flag: bool) -> Result<(), SafetyError> {
    if binding && PROTECTED_PORTS.contains(&port) && !override_flag {
        return Err(SafetyError::ProtectedPort(port));
    }
    Ok(())
}

/// Port gate. `port` is the port about to be bound by GNOUS's own relay.
///
/// Binding a protected port from inside GNOUS is how this session would die, so
/// it is refused unless the process was started with [`ALLOW_PROTECTED_ENV`]
/// (which only the migrated unit sets). A read-only lookup (`binding = false`)
/// is always allowed — that is what status needs.
pub fn check_port(port: u16, binding: bool) -> Result<(), SafetyError> {
    check_port_with(port, binding, protected_override())
}

fn is_legacy(unit: &str) -> bool {
    LEGACY_UNITS
        .iter()
        .any(|candidate| unit == *candidate || unit == candidate.trim_end_matches(".service"))
}

/// `systemctl --user <verb> <unit>` gate.
///
/// `argv` is the full argument list *after* `systemctl`.
pub fn check_systemctl(argv: &[String]) -> Result<(), SafetyError> {
    let mut verb = None;
    let mut units = Vec::new();
    for arg in argv {
        let trimmed = arg.trim_start_matches('-');
        if arg.starts_with('-') {
            // Long options carry no unit names; short ones carry no verbs here.
            continue;
        }
        if verb.is_none() && READONLY_VERBS.contains(&trimmed) {
            verb = Some(trimmed.to_string());
            continue;
        }
        if verb.is_none() && OWN_UNIT_VERBS.contains(&trimmed) {
            verb = Some(trimmed.to_string());
            continue;
        }
        if verb.is_none() && FORBIDDEN_VERBS.contains(&trimmed) {
            return Err(SafetyError::ForbiddenVerb(trimmed.to_string()));
        }
        units.push(trimmed.to_string());
    }
    let verb = verb.ok_or_else(|| SafetyError::NotAllowlisted(argv.join(" ")))?;
    if FORBIDDEN_VERBS.contains(&verb.as_str()) {
        return Err(SafetyError::ForbiddenVerb(verb));
    }
    // Read verbs are allowed for any unit: the whole point of the panel is to
    // report on the legacy relay without owning it.
    if READONLY_VERBS.contains(&verb.as_str()) {
        return Ok(());
    }
    for unit in &units {
        if is_legacy(unit) {
            return Err(SafetyError::ForeignUnit(unit.clone()));
        }
        if OWN_UNIT_VERBS.contains(&verb.as_str()) && unit != OWN_UNIT {
            return Err(SafetyError::ForeignUnit(unit.clone()));
        }
    }
    if OWN_UNIT_VERBS.contains(&verb.as_str()) && units.is_empty() {
        return Err(SafetyError::NotAllowlisted(argv.join(" ")));
    }
    Ok(())
}

/// Non-systemctl commands the backend is allowed to run, verbatim.
pub const ALL_COMMANDS: &[&[&str]] = &[
    &["ip", "-4", "-o", "addr", "show", "scope", "global"],
    &[
        "systemctl",
        "--user",
        "show",
        LEGACY_UNITS[0],
        "-p",
        "LoadState,ActiveState,SubState,UnitFileState,ExecMainPID,ExecStart,NRestarts",
    ],
    &["systemctl", "--user", "cat", LEGACY_UNITS[0]],
    &[
        "systemctl",
        "--user",
        "show",
        OWN_UNIT,
        "-p",
        "LoadState,ActiveState,SubState,UnitFileState,ExecMainPID,ExecStart,NRestarts",
    ],
    &["systemctl", "--user", "is-active", OWN_UNIT],
    &["systemctl", "--user", "is-enabled", OWN_UNIT],
    &["systemctl", "--user", "start", OWN_UNIT],
    &["systemctl", "--user", "stop", OWN_UNIT],
    &["systemctl", "--user", "enable", OWN_UNIT],
    &["systemctl", "--user", "disable", OWN_UNIT],
    &["systemctl", "--user", "show-environment"],
    &["gsettings", "get", "org.gnome.system.proxy", "mode"],
    &["gsettings", "get", "org.gnome.system.proxy", "ignore-hosts"],
];

/// Single choke point: every spawn in this module must pass here first.
pub fn check_command(argv: &[&str]) -> Result<(), SafetyError> {
    let joined: Vec<String> = argv.iter().map(|arg| (*arg).to_string()).collect();
    check_command_owned(&joined)
}

pub fn check_command_owned(argv: &[String]) -> Result<(), SafetyError> {
    let Some(program) = argv.first() else {
        return Err(SafetyError::NotAllowlisted(String::new()));
    };
    check_program(program)?;
    let matches_allowlist = ALL_COMMANDS.iter().any(|allowed| {
        allowed.len() == argv.len() && allowed.iter().zip(argv).all(|(a, b)| *a == b)
    });
    if program == "systemctl" {
        let rest: Vec<String> = argv[1..].to_vec();
        check_systemctl(&rest)?;
    }
    if program == "gsettings" {
        // Read-only by construction: `set` / `reset` would rewire the desktop's
        // proxy and is emitted as a copyable command instead.
        if argv.get(1).map(String::as_str) != Some("get") {
            return Err(SafetyError::ForbiddenVerb(
                argv.get(1).cloned().unwrap_or_default(),
            ));
        }
    }
    if matches_allowlist {
        Ok(())
    } else {
        Err(SafetyError::NotAllowlisted(argv.join(" ")))
    }
}

/// `GNOUS_PROXY_DRY_RUN=1` turns every write into a printable plan. The
/// development of this extension runs with it set.
pub fn dry_run() -> bool {
    matches!(
        std::env::var("GNOUS_PROXY_DRY_RUN").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn never_mutates_the_legacy_relay() {
        for verb in ["start", "stop", "restart", "disable", "enable"] {
            let err = check_systemctl(&argv(&[verb, "j1900-proxy.service"]))
                .expect_err("mutation of the legacy unit must be refused");
            assert!(
                matches!(
                    err,
                    SafetyError::ForeignUnit(_) | SafetyError::ForbiddenVerb(_)
                ),
                "{verb} leaked through: {err:?}"
            );
        }
    }

    #[test]
    fn session_breaking_verbs_are_always_refused() {
        for verb in [
            "set-environment",
            "unset-environment",
            "daemon-reload",
            "kill",
            "reload",
        ] {
            assert!(matches!(
                check_systemctl(&argv(&[verb, OWN_UNIT])),
                Err(SafetyError::ForbiddenVerb(_))
            ));
        }
        // Even without a unit argument, the verb alone is refused.
        assert!(matches!(
            check_systemctl(&argv(&["unset-environment", "http_proxy"])),
            Err(SafetyError::ForbiddenVerb(_))
        ));
    }

    #[test]
    fn only_our_own_unit_can_be_mutated() {
        assert!(check_systemctl(&argv(&["start", OWN_UNIT])).is_ok());
        assert!(check_systemctl(&argv(&["stop", OWN_UNIT])).is_ok());
        for unit in ["gnome-shell.service", "pipewire.service", "sshd.service"] {
            assert!(matches!(
                check_systemctl(&argv(&["start", unit])),
                Err(SafetyError::ForeignUnit(_))
            ));
        }
    }

    #[test]
    fn reads_are_allowed_for_any_unit() {
        assert!(
            check_systemctl(&argv(&["show", "j1900-proxy.service", "-p", "ActiveState"])).is_ok()
        );
        assert!(check_systemctl(&argv(&["is-active", OWN_UNIT])).is_ok());
    }

    #[test]
    fn gsettings_can_only_read() {
        assert!(matches!(
            check_command(&["gsettings", "set", "org.gnome.system.proxy", "mode", "none"]),
            Err(SafetyError::ForbiddenVerb(_))
        ));
        assert!(matches!(
            check_command(&["gsettings", "get", "org.gnome.system.proxy", "mode"]),
            Ok(())
        ));
        assert!(matches!(
            check_command(&["bash"]),
            Err(SafetyError::ForeignProgram(_))
        ));
    }

    #[test]
    fn binding_a_protected_port_needs_an_explicit_opt_in() {
        for port in PROTECTED_PORTS {
            assert!(matches!(
                check_port_with(*port, true, false),
                Err(SafetyError::ProtectedPort(p)) if p == *port
            ));
            assert!(check_port_with(*port, false, false).is_ok());
            // The installed unit passes the flag after a deliberate handover.
            assert!(check_port_with(*port, true, true).is_ok());
        }
        assert!(check_port_with(31299, true, false).is_ok());
    }

    #[test]
    fn every_allowlisted_command_passes_the_choke_point() {
        for allowed in ALL_COMMANDS {
            check_command_owned(&argv(allowed))
                .unwrap_or_else(|error| panic!("{allowed:?} 被闸门拒绝：{error}"));
        }
    }

    #[test]
    fn allowlist_entries_never_name_the_protected_ports_or_legacy_mutations() {
        for allowed in ALL_COMMANDS {
            let joined = allowed.join(" ");
            for port in PROTECTED_PORTS {
                assert!(
                    !joined.contains(&format!(":{port}")),
                    "allowlist entry binds protected port: {joined}"
                );
            }
            assert!(!joined.contains("set-environment"), "{joined}");
            assert!(!joined.contains("unset-environment"), "{joined}");
        }
    }

    #[test]
    fn unlisted_commands_are_refused() {
        assert!(matches!(
            check_command(&["ip", "-4", "addr", "flush"]),
            Err(SafetyError::NotAllowlisted(_))
        ));
        assert!(matches!(
            check_command(&["systemctl", "--user", "show", OWN_UNIT, "-p", "Nothing"]),
            Err(SafetyError::NotAllowlisted(_))
        ));
    }
}
