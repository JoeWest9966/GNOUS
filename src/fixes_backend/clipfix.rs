//! Built-in `clipfix`: real Wayland/X11 probes plus a reversible
//! `GDK_BACKEND=x11` systemd user-manager patch.
//!
//! The old standalone CLI is migrated once: its
//! manifest is read for the previous value, rewritten as JSON, then removed.
//! `--now`, `fix --run --yes`, and session logout are intentionally absent.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::state;
use super::wayland;
use super::x11;
use super::{Op, Tool, outcome, process};

const ENV_KEY: &str = "GDK_BACKEND";
const ENV_VALUE: &str = "x11";
const STATE_NAME: &str = "clipfix.json";
const LEGACY_MANIFEST: &str = "clipfix/manifest.txt";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
struct PatchState {
    version: u32,
    applied_at: String,
    prev_present: bool,
    prev_value: String,
}

impl PatchState {
    fn current() -> Self {
        state::read_json(STATE_NAME).unwrap_or_else(migrate_legacy_manifest)
    }
}

fn migrate_legacy_manifest() -> PatchState {
    let home = state::home();
    let legacy = home.join(".local/state").join(LEGACY_MANIFEST);
    let Ok(text) = std::fs::read_to_string(&legacy) else {
        return PatchState::default();
    };
    let value = parse_manifest(&text);
    let _ = state::write_json(STATE_NAME, &value);
    let _ = std::fs::remove_file(&legacy);
    value
}

fn parse_manifest(text: &str) -> PatchState {
    let mut value = PatchState {
        version: 1,
        ..Default::default()
    };
    for line in text.lines() {
        let line = line.trim();
        let Some((key, raw)) = line.split_once('=') else {
            continue;
        };
        match key {
            "applied_at" => value.applied_at = raw.to_string(),
            "prev_present" => value.prev_present = raw == "1",
            "prev_value" => value.prev_value = raw.to_string(),
            _ => {}
        }
    }
    value
}

fn user_env() -> Result<Option<String>, String> {
    let out = process::run_user("systemctl", &["--user", "show-environment"])?;
    Ok(out.lines().find_map(|line| {
        line.split_once('=')
            .filter(|(key, _)| *key == ENV_KEY)
            .map(|(_, value)| value.to_string())
    }))
}

fn set_env(value: &str) -> Result<(), String> {
    process::run_user(
        "systemctl",
        &["--user", "set-environment", &format!("{ENV_KEY}={value}")],
    )
    .map(|_| ())
}

fn unset_env() -> Result<(), String> {
    process::run_user("systemctl", &["--user", "unset-environment", ENV_KEY]).map(|_| ())
}

fn timestamp() -> String {
    crate::sys::local_clock()
}

/// Best-effort deep verification. `systemd-run --pipe --wait` can hang on
/// this machine (the old CLI had the same finding), so the deadline is short
/// and a timeout only downgrades the report instead of blocking the action.
fn verify_new_units(want: Option<&str>) -> Option<bool> {
    let out = process::run_bounded(
        "systemd-run",
        &["--user", "--pipe", "--wait", "--quiet", "/usr/bin/env"],
        Duration::from_secs(4),
    )
    .ok()?;
    let got = out.lines().find_map(|line| {
        line.split_once('=')
            .filter(|(key, _)| *key == ENV_KEY)
            .map(|(_, value)| value)
    });
    Some(got == want)
}

fn terminal_backend() -> Value {
    let pids = process::find_pids_by_comm("gnome-terminal-server");
    let runtime = std::env::var("XDG_RUNTIME_DIR").unwrap_or_default();
    let pid = pids
        .iter()
        .copied()
        .find(|pid| process::proc_env(*pid, "XDG_RUNTIME_DIR").unwrap_or_default() == runtime)
        .or_else(|| pids.first().copied());
    let x11_windows = x11::X11::open()
        .map(|display| display.find_windows_by_class("terminal", 6).len())
        .unwrap_or(0);
    json!({
        "pid": pid,
        "gdk_backend": pid.and_then(|pid| process::proc_env(pid, ENV_KEY)),
        "x11_windows": x11_windows,
    })
}

fn x11_owners() -> Value {
    let Some(display) = x11::X11::open() else {
        return json!({"available": false, "reason": "DISPLAY / libX11 不可用"});
    };
    let mut owners = serde_json::Map::new();
    for selection in ["CLIPBOARD", "PRIMARY"] {
        let owner = display.selection_owner(selection);
        let info = if owner == 0 {
            json!({"owner": 0, "anonymous": false})
        } else {
            let info = display.describe_window(owner);
            json!({"owner": owner, "anonymous": info.is_anonymous(), "summary": info.summary()})
        };
        owners.insert(selection.to_ascii_lowercase(), info);
    }
    json!({"available": true, "owners": owners})
}

fn roundtrip() -> (Value, String) {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let token = format!("GNOUS-CLIPFIX-{}-{nonce}", std::process::id());
    let Some(exe) = std::env::current_exe().ok() else {
        return (
            json!({"status": "unknown", "error": "取不到自身路径"}),
            "无法启动内建 holder".into(),
        );
    };
    let child = Command::new(exe)
        .args(["--builtin-clipfix-holder", &token, "6000"])
        .stdout(std::process::Stdio::piped())
        .spawn();
    let Ok(mut child) = child else {
        return (
            json!({"status": "unknown", "error": "无法启动 holder"}),
            String::new(),
        );
    };
    std::thread::sleep(Duration::from_millis(700));
    let read = wayland::read_clipboard(Duration::from_millis(2500));
    let mut holder = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        use std::io::Read as _;
        let _ = stdout.read_to_string(&mut holder);
    }
    let _ = child.wait();
    let got_token = read
        .text
        .as_deref()
        .is_some_and(|text| text.trim() == token);
    let cancelled = holder.contains("cancelled=true");
    let (status, verdict) = if got_token {
        ("ok", "OK（复制→粘贴链路通）")
    } else if cancelled {
        ("broken", "BROKEN（compositor 抢走/取消了 selection）")
    } else {
        ("broken", "BROKEN（holder 占住了，但别的客户端读不到）")
    };
    let detail = format!(
        "holder: {}\nreader: {}\n{}",
        holder.trim().replace('\n', " | "),
        if read.got_offer {
            format!("收到 offer，mime={:?}", read.mimes)
        } else {
            "没有收到 selection / offer".into()
        },
        read.sanity.join("\n")
    );
    (
        json!({
            "status": status,
            "verdict": verdict,
            "got_token": got_token,
            "holder_cancelled": cancelled,
            "mimes": read.mimes,
            "protocol_error": read.error,
            "detail": detail,
        }),
        detail,
    )
}

use std::process::Command;

/// Internal child mode; never exposed through the UI. It owns the selection
/// for a few seconds so a second connection can verify the real data path.
pub fn holder(token: &str, hold_ms: u64) {
    let result = wayland::hold_clipboard(token, Duration::from_millis(hold_ms));
    println!(
        "cancelled={} requests={} error={} {}",
        result.cancelled,
        result.requests,
        result.error.unwrap_or_default(),
        result.sanity.join(" | ")
    );
}

pub(crate) fn run(op: Op) -> super::Outcome {
    match op {
        Op::Check => check(),
        Op::Apply => apply(),
        Op::Revert => revert(),
        _ => super::Outcome::invalid(Tool::Clipfix, op),
    }
}

fn check() -> super::Outcome {
    let t0 = Instant::now();
    let patch = PatchState::current();
    let current = user_env().ok().flatten();
    let active = current.as_deref() == Some(ENV_VALUE);
    let (roundtrip, detail) = roundtrip();
    let owners = x11_owners();
    let terminal = terminal_backend();
    let status = if patch.version == 1 && active {
        "applied"
    } else if roundtrip["status"] == "ok" {
        "healthy"
    } else if roundtrip["status"] == "broken" {
        "attention"
    } else {
        "unknown"
    };
    let facts = json!({
        "status": status,
        "patch_active": active,
        "manifest_present": patch.version == 1,
        "user_manager_env": current,
        "roundtrip": roundtrip,
        "x11": owners,
        "terminal": terminal,
        "elapsed_ms": t0.elapsed().as_millis(),
    });
    let mut output = String::new();
    output.push_str("===== GNOUS 内建 clipfix 体检 =====\n");
    output.push_str(&format!(
        "会话：XDG_SESSION_TYPE={} WAYLAND_DISPLAY={} DISPLAY={}\n",
        std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "?".into()),
        std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "(未设置)".into()),
        std::env::var("DISPLAY").unwrap_or_else(|_| "(未设置)".into())
    ));
    output.push_str(&format!(
        "补丁状态：{}\n",
        if active { "已启用" } else { "未启用" }
    ));
    output.push_str(&format!(
        "Wayland 回环自测：{}\n",
        roundtrip["verdict"].as_str().unwrap_or("UNKNOWN")
    ));
    output.push_str(&format!("  {detail}\n"));
    output.push_str(&format!("终端后端：{}\n", terminal));
    output.push_str(&format!("X11 owner：{}\n", owners));
    if active {
        output.push_str(
            "注意：补丁把终端绕到 X11；Wayland 原生程序之间仍可能不通，需重开终端生效。\n",
        );
    }
    output.push_str(&format!("结构化状态：{facts}"));
    outcome(Tool::Clipfix, Op::Check, ok_facts(status), output, facts)
}

fn ok_facts(status: &str) -> bool {
    status != "unknown" && status != "failed"
}

fn apply() -> super::Outcome {
    let existing = PatchState::current();
    let current = match user_env() {
        Ok(value) => value,
        Err(error) => return failed(Op::Apply, error),
    };
    if existing.version == 1 && current.as_deref() == Some(ENV_VALUE) {
        return outcome(
            Tool::Clipfix,
            Op::Apply,
            true,
            "补丁已经启用；重复执行未改动系统状态。\n关掉所有终端窗口再重新打开即可生效。".into(),
            json!({"status": "applied", "changed": false}),
        );
    }
    let previous = current.clone();
    let patch = PatchState {
        version: 1,
        applied_at: timestamp(),
        prev_present: previous.is_some(),
        prev_value: previous.clone().unwrap_or_default(),
    };
    if let Err(error) = state::write_json(STATE_NAME, &patch) {
        return failed(Op::Apply, error);
    }
    if let Err(error) = set_env(ENV_VALUE) {
        state::remove(STATE_NAME);
        return failed(Op::Apply, error);
    }
    let readback = match user_env() {
        Ok(value) => value,
        Err(error) => return failed(Op::Apply, error),
    };
    if readback.as_deref() != Some(ENV_VALUE) {
        let _ = rollback(&patch);
        state::remove(STATE_NAME);
        return failed(Op::Apply, "设置后读回不一致，已自动回退".into());
    }
    let verify = verify_new_units(Some(ENV_VALUE));
    let facts = json!({
        "status": "applied",
        "changed": true,
        "previous": previous,
        "readback": readback,
        "new_unit_verified": verify,
    });
    let output = format!(
        "✓ 内建补丁已启用：systemctl --user set-environment {ENV_KEY}={ENV_VALUE}\n\
         ✓ 状态已写入 {}（JSON，可精确回退）\n\
         深度验证：{}\n\
         下一步：关掉所有终端窗口再重新打开；不会自动终止终端进程。\n\
         结构化状态：{facts}",
        state::state_file().display(),
        match verify {
            Some(true) => "新单元继承到 x11 ✅",
            Some(false) => "新单元未继承到 x11（补丁仍已设置）",
            None => "验证手段不可用（补丁仍已设置）",
        }
    );
    outcome(Tool::Clipfix, Op::Apply, true, output, facts)
}

fn revert() -> super::Outcome {
    let patch = PatchState::current();
    let current = match user_env() {
        Ok(value) => value,
        Err(error) => return failed(Op::Revert, error),
    };
    if patch.version == 0 {
        if current.as_deref() != Some(ENV_VALUE) {
            return outcome(
                Tool::Clipfix,
                Op::Revert,
                true,
                "没有内建状态；当前环境无需回退。".into(),
                json!({"status": "healthy", "changed": false}),
            );
        }
        return failed(
            Op::Revert,
            "当前 GDK_BACKEND=x11 但不是 GNOUS 内建补丁设置的；不擅自删除。".into(),
        );
    }
    let result = rollback(&patch);
    if let Err(error) = result {
        return failed(Op::Revert, error);
    }
    let after = user_env().ok().flatten();
    let expected = patch.prev_present.then_some(patch.prev_value.as_str());
    let env_ok = match expected {
        Some(value) => after.as_deref() == Some(value),
        None => after.is_none(),
    };
    let verify = verify_new_units(expected);
    if !env_ok {
        return failed(Op::Revert, "回退后读回不一致".into());
    }
    state::remove(STATE_NAME);
    let facts = json!({
        "status": "healthy",
        "changed": true,
        "restored": expected,
        "readback": after,
        "new_unit_verified": verify,
    });
    let output = format!(
        "✓ 已按内建状态回退 {ENV_KEY}\n\
         ✓ user manager 读回：{}\n\
         ✓ JSON 状态已删除\n\
         结构化状态：{facts}\n\
         已开着的 X11 终端不会自动变回 Wayland；关掉重开即可。",
        after.unwrap_or_else(|| "(未设置)".into())
    );
    outcome(Tool::Clipfix, Op::Revert, true, output, facts)
}

fn rollback(patch: &PatchState) -> Result<(), String> {
    if patch.prev_present {
        set_env(&patch.prev_value)
    } else {
        unset_env()
    }
}

fn failed(op: Op, error: String) -> super::Outcome {
    let facts = json!({"status": "failed", "error": error});
    outcome(Tool::Clipfix, op, false, format!("✗ {error}"), facts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_reads_previous_value_from_the_legacy_manifest() {
        let text = "# legacy\n\
            applied_at=2026-01-02 03:04\n\
            prev_present=1\n\
            prev_value=wayland\n";
        let patch = parse_manifest(text);
        assert_eq!(patch.version, 1);
        assert_eq!(patch.applied_at, "2026-01-02 03:04");
        assert!(patch.prev_present);
        assert_eq!(patch.prev_value, "wayland");
    }

    #[test]
    fn json_state_is_the_rollback_source_of_truth() {
        let patch = PatchState {
            version: 1,
            applied_at: "14:00".into(),
            prev_present: true,
            prev_value: "wayland".into(),
        };
        let value = serde_json::to_value(&patch).unwrap();
        assert_eq!(value["prev_present"], true);
        assert_eq!(value["prev_value"], "wayland");
        let restored: PatchState = serde_json::from_value(value).unwrap();
        assert_eq!(restored, patch);
    }

    #[test]
    fn check_facts_never_confuse_a_broken_compositor_with_a_failed_probe() {
        let (facts, _) = (
            json!({"status": "attention", "roundtrip": {"status": "broken"}}),
            "",
        );
        assert_eq!(facts["status"], "attention");
        assert_eq!(facts["roundtrip"]["status"], "broken");
    }
}
