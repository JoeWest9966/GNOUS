//! The single execution seam for this extension.
//!
//! Every external command goes through [`run`], which first asks
//! [`super::guard`] whether the command is allowed. There is no other `Command`
//! in this module tree, so "accidentally shelling out to something dangerous"
//! is a compile-and-review-visible change rather than a silent possibility.

use std::time::Duration;

use crate::fixes_backend::process;

use super::guard;

const TIMEOUT: Duration = Duration::from_secs(6);

/// Run an allowlisted read-only command, bounded in time and output.
pub fn run(argv: &[&str]) -> Result<String, String> {
    let owned: Vec<String> = argv.iter().map(|arg| (*arg).to_string()).collect();
    guard::check_command_owned(&owned).map_err(|error| error.to_string())?;
    let (program, rest) = owned.split_first().ok_or("空命令")?;
    let borrowed: Vec<&str> = rest.iter().map(String::as_str).collect();
    process::run_bounded(program, &borrowed, TIMEOUT)
}

/// Read a systemd property set for `unit` (read-only; allowed for any unit).
pub fn unit_properties(unit: &str) -> Result<String, String> {
    run(&[
        "systemctl",
        "--user",
        "show",
        unit,
        "-p",
        "LoadState,ActiveState,SubState,UnitFileState,ExecMainPID,ExecStart,NRestarts",
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_before_spawning() {
        let error = run(&["systemctl", "--user", "stop", "j1900-proxy.service"]).unwrap_err();
        assert!(
            error.contains("外部单元") || error.contains("拒绝"),
            "{error}"
        );
        let error = run(&["rm", "-rf", "/tmp"]).unwrap_err();
        assert!(
            error.contains("允许清单") || error.contains("可执行程序"),
            "{error}"
        );
    }

    #[test]
    fn read_only_systemctl_is_allowed_but_must_be_allowlisted() {
        // Not in ALL_COMMANDS → refused by the allowlist layer, still before spawn.
        let error = run(&["systemctl", "--user", "show", "gnous-proxy-relay.service"]).unwrap_err();
        assert!(error.contains("允许清单"), "{error}");
    }
}
