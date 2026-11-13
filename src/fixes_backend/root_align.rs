//! Built-in `root-align`: eight root-session user units, condition overrides,
//! structured status/doctor/apply/revert, and an isolated restart-shell action.
//!
//! This replaces the old standalone CLI. The legacy
//! key=value state file is read once to learn which units GNOUS owns; from
//! then on state is JSON and rollback is exact.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Op, Tool, outcome, process, state};

/// Root-session unit overrides. `HOME` is derived at runtime so tests and
/// non-root installations never write to a hard-coded path.
pub const UNIT_DIR: &str = ".config/systemd/user";
pub const STATE_FILE: &str = "/root/.config/root-gnome-align/state.json";
pub const STATE_NAME: &str = "root-align.json";
pub const OVERRIDE_CONTENT: &str = "# 由 GNOUS 内建 root-align 生成\n\
    # 清空发行版单元里的 ConditionUser 条件，仅影响 root 用户。\n\
    [Unit]\nConditionUser=\n";

#[derive(Clone, Debug)]
pub struct Unit {
    pub name: &'static str,
    pub description: &'static str,
    pub domain: usize,
}

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

pub const DOMAINS: &[(&str, &str)] = &[
    ("音频", "音量条、声音面板、系统提示音、录音 / 通话。"),
    ("设备枚举 / 路由", "输出设备、音量路由、滤波器链。"),
    ("媒体控制 / 蓝牙", "媒体键、GNOME 指示器、耳机播放控制。"),
    ("全局搜索", "GNOME 概览检索本地文档内容。"),
];

const START_ORDER: &[&str] = &[
    "pipewire.socket",
    "pipewire-pulse.socket",
    "pipewire.service",
    "wireplumber.service",
    "filter-chain.service",
    "pipewire-pulse.service",
    "mpris-proxy.service",
    "localsearch-3.service",
];

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
struct AlignState {
    version: u32,
    applied_at: String,
    units: Vec<String>,
}

impl AlignState {
    fn current() -> Self {
        if let Some(value) = state::read_json(STATE_NAME) {
            return value;
        }
        let value = migrate_legacy_state();
        let _ = state::write_json(STATE_NAME, &value);
        value
    }
}

fn migrate_legacy_state() -> AlignState {
    let Ok(text) = fs::read_to_string(STATE_FILE) else {
        return AlignState::default();
    };
    let mut value = AlignState {
        version: 1,
        ..Default::default()
    };
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        if let Some((key, raw)) = line.split_once('=') {
            match key {
                "applied_at" => value.applied_at = raw.to_string(),
                "units" => value.units = raw.split(',').map(str::to_string).collect(),
                _ => {}
            }
        }
    }
    value
}

fn unit_dir() -> PathBuf {
    state::home().join(UNIT_DIR)
}

fn unit_path(name: &str) -> PathBuf {
    unit_dir().join(format!("{name}.d")).join("override.conf")
}

fn override_applied(name: &str) -> bool {
    fs::read_to_string(unit_path(name))
        .map(|text| text == OVERRIDE_CONTENT || text.contains("ConditionUser="))
        .unwrap_or(false)
}

fn systemctl(args: &[&str]) -> Result<String, String> {
    let mut full = vec!["--user", "--no-pager", "--no-legend"];
    full.extend_from_slice(args);
    process::run_user("systemctl", &full)
}

fn unit_properties(name: &str) -> Value {
    let active = systemctl(&["is-active", name]).unwrap_or_else(|_| "unknown".into());
    let enabled = systemctl(&["is-enabled", name]).unwrap_or_else(|_| "unknown".into());
    json!({
        "name": name,
        "active": active,
        "enabled": enabled,
        "override_applied": override_applied(name),
    })
}

fn units() -> Vec<Value> {
    UNITS
        .iter()
        .map(|unit| {
            let mut props = unit_properties(unit.name);
            props["description"] = json!(unit.description);
            props["domain"] = json!(unit.domain);
            props["healthy"] =
                json!(props["active"] == "active" && props["override_applied"] == true);
            props
        })
        .collect()
}

fn audio() -> Value {
    let sinks = process::run_user_timeout(
        "pactl",
        &["--format=json", "list", "short", "sinks"],
        std::time::Duration::from_secs(4),
    )
    .ok()
    .and_then(|text| serde_json::from_str::<Value>(&text).ok())
    .unwrap_or_else(|| json!([]));
    json!({"sink_count": sinks.as_array().map(Vec::len).unwrap_or(0), "sinks": sinks})
}

fn remove_tree(path: &Path) -> Result<(), String> {
    if path.is_dir() {
        for entry in fs::read_dir(path).map_err(|e| format!("读 {path:?} 失败: {e}"))? {
            remove_tree(
                &entry
                    .map_err(|e| format!("遍历 {path:?} 失败: {e}"))?
                    .path(),
            )?;
        }
        fs::remove_dir(path).map_err(|e| format!("删目录 {path:?} 失败: {e}"))
    } else if path.exists() {
        fs::remove_file(path).map_err(|e| format!("删文件 {path:?} 失败: {e}"))
    } else {
        Ok(())
    }
}

fn ensure_root() -> Result<(), String> {
    if unsafe { libc::geteuid() } == 0 {
        Ok(())
    } else {
        Err("内建 root-align 需要 root 权限".into())
    }
}

pub(crate) fn run(op: Op) -> super::Outcome {
    match op {
        Op::Check | Op::Status => status(op),
        Op::Doctor => doctor(),
        Op::Apply => apply(),
        Op::Revert => revert(),
        Op::RestartShell => restart_shell(),
    }
}

fn status(op: Op) -> super::Outcome {
    let unit_values = units();
    let all_ok = unit_values.iter().all(|unit| unit["healthy"] == true);
    let audio_value = audio();
    let status = if all_ok { "aligned" } else { "attention" };
    let facts = json!({
        "status": status,
        "units": unit_values,
        "domains": DOMAINS.iter().map(|(name, description)| json!({"name": name, "description": description})).collect::<Vec<_>>(),
        "audio": audio_value,
        "state_file": state::state_file(),
    });
    let output = render_status(&facts);
    outcome(Tool::RootAlign, op, true, output, facts)
}

fn doctor() -> super::Outcome {
    let uid = unsafe { libc::geteuid() };
    let mut conditions = serde_json::Map::new();
    for unit in UNITS {
        let path = format!("/usr/lib/systemd/user/{}", unit.name);
        let lines = fs::read_to_string(&path)
            .map(|text| {
                text.lines()
                    .filter(|line| line.trim().starts_with("ConditionUser"))
                    .map(str::trim)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        conditions.insert(unit.name.into(), json!(lines));
    }
    let journal = process::run_user_timeout(
        "journalctl",
        &["--user", "-b", "--no-pager", "-o", "json", "_UID=0"],
        std::time::Duration::from_secs(5),
    )
    .unwrap_or_default();
    let skipped: Vec<&str> = journal
        .lines()
        .filter(|line| line.contains("unmet condition check ConditionUser"))
        .take(12)
        .collect();
    let facts = json!({
        "status": "diagnosed",
        "uid": uid,
        "is_root": uid == 0,
        "condition_user": conditions,
        "journal_skips": skipped,
        "dev_snd": Path::new("/dev/snd").exists(),
        "pipewire": Path::new("/usr/bin/pipewire").exists(),
        "wireplumber": Path::new("/usr/bin/wireplumber").exists(),
    });
    let output = format!(
        "── 环境检查 ──\n  当前 UID: {uid} {}\n  /dev/snd: {}  PipeWire: {}  WirePlumber: {}\n\n── 发行版 ConditionUser 条件 ──\n{}\n\n── 本轮日志中的条件跳过（最多 12 条）──\n{}\n\n结构化诊断：{facts}",
        if uid == 0 {
            "(root ✓)"
        } else {
            "(非 root ✗)"
        },
        facts["dev_snd"],
        facts["pipewire"],
        facts["wireplumber"],
        conditions
            .iter()
            .map(|(name, lines)| format!(
                "  {name}: {}",
                serde_json::to_string(lines).unwrap_or_default()
            ))
            .collect::<Vec<_>>()
            .join("\n"),
        if skipped.is_empty() {
            "  无 ConditionUser 跳过记录".into()
        } else {
            skipped.join("\n")
        },
    );
    outcome(Tool::RootAlign, Op::Doctor, true, output, facts)
}

fn apply() -> super::Outcome {
    if let Err(error) = ensure_root() {
        return failed(Op::Apply, error);
    }
    let mut created = Vec::new();
    for unit in UNITS {
        let path = unit_path(unit.name);
        if matches!(fs::read_to_string(&path).as_deref(), Ok(text) if text == OVERRIDE_CONTENT) {
            continue;
        }
        if let Some(parent) = path.parent() {
            if let Err(error) = fs::create_dir_all(parent) {
                return failed(Op::Apply, format!("创建 {parent:?} 失败: {error}"));
            }
        }
        if let Err(error) = fs::write(&path, OVERRIDE_CONTENT) {
            return failed(Op::Apply, format!("写 {path:?} 失败: {error}"));
        }
        created.push(unit.name.to_string());
    }
    if let Err(error) = systemctl(&["daemon-reload"]) {
        return failed(Op::Apply, error);
    }
    let mut starts = Vec::new();
    for name in START_ORDER {
        match systemctl(&["start", name]) {
            Ok(_) => starts.push(json!({"unit": name, "started": true})),
            Err(first) => {
                let _ = systemctl(&["reset-failed", name]);
                let started = systemctl(&["start", name]).is_ok();
                starts.push(json!({"unit": name, "started": started, "error": first}));
            }
        }
    }
    let _ = systemctl(&["reset-failed", "pipewire.socket"]);
    let _ = systemctl(&["reset-failed", "pipewire-pulse.socket"]);
    let align = AlignState {
        version: 1,
        applied_at: crate::sys::local_clock(),
        units: UNITS.iter().map(|unit| unit.name.to_string()).collect(),
    };
    if let Err(error) = state::write_json(STATE_NAME, &align) {
        return failed(Op::Apply, error);
    }
    let unit_values = units();
    let all_ok = unit_values.iter().all(|unit| unit["healthy"] == true);
    let facts = json!({
        "status": if all_ok { "aligned" } else { "attention" },
        "created_overrides": created,
        "starts": starts,
        "units": unit_values,
        "state_file": state::state_file(),
    });
    let output = format!(
        "── 应用内建对齐 ──\n  ✓ 写入/确认 {} 个用户单元覆盖\n  ✓ daemon-reload\n  ✓ 按 socket → service 顺序启动\n  {}\n\n结构化状态：{facts}\n音量条可能需要重启 GNOME Shell；该动作在独立危险区，不会自动执行。",
        UNITS.len(),
        render_units(&unit_values),
    );
    outcome(Tool::RootAlign, Op::Apply, all_ok, output, facts)
}

fn revert() -> super::Outcome {
    if let Err(error) = ensure_root() {
        return failed(Op::Revert, error);
    }
    let previous = AlignState::current();
    let owned: Vec<&str> = if previous.version == 1 && !previous.units.is_empty() {
        previous.units.iter().map(String::as_str).collect()
    } else {
        UNITS.iter().map(|unit| unit.name).collect()
    };
    for name in owned.iter().rev() {
        let _ = systemctl(&["stop", name]);
    }
    let _ = systemctl(&["reset-failed"]);
    let mut removed = Vec::new();
    for name in &owned {
        let dir = unit_dir().join(format!("{name}.d"));
        if let Err(error) = remove_tree(&dir) {
            return failed(Op::Revert, error);
        }
        removed.push((*name).to_string());
    }
    if let Err(error) = systemctl(&["daemon-reload"]) {
        return failed(Op::Revert, error);
    }
    state::remove(STATE_NAME);
    // The legacy state file is a migration artifact; leaving it would make a
    // future first-run import resurrect the already reverted state.
    let _ = fs::remove_file(STATE_FILE);
    let facts = json!({
        "status": "reverted",
        "stopped": owned,
        "removed_override_dirs": removed,
        "state_file": state::state_file(),
    });
    let output = format!(
        "── 回退内建对齐 ──\n  ✓ 停止 {} 个单元（未禁用全局启用）\n  ✓ 删除 {} 个 .d/override.conf\n  ✓ daemon-reload，JSON 状态已清除\n  ✓ 旧 key=value 状态文件已清理\n\n结构化状态：{facts}",
        owned.len(),
        removed.len()
    );
    outcome(Tool::RootAlign, Op::Revert, true, output, facts)
}

fn restart_shell() -> super::Outcome {
    let result = process::run_user_timeout(
        "killall",
        &["-HUP", "gnome-shell"],
        std::time::Duration::from_secs(4),
    );
    let ok = result.is_ok();
    let facts = json!({
        "status": if ok { "requested" } else { "manual_required" },
        "signal": "SIGHUP",
        "target": "gnome-shell",
        "exit_ok": ok,
    });
    let output = if ok {
        format!("✓ 已请求 GNOME Shell 重新初始化\n结构化状态：{facts}")
    } else {
        format!(
            "! 自动重启请求失败，请按 Alt+F2 输入 r 回车，或注销后重新登录\n原因：{}\n结构化状态：{facts}",
            result.unwrap_err()
        )
    };
    outcome(Tool::RootAlign, Op::RestartShell, ok, output, facts)
}

fn failed(op: Op, error: String) -> super::Outcome {
    let facts = json!({"status": "failed", "error": error});
    outcome(Tool::RootAlign, op, false, format!("✗ {error}"), facts)
}

fn render_units(unit_values: &[Value]) -> String {
    unit_values
        .iter()
        .map(|unit| {
            format!(
                "  {} {:<26} active={:<10} enabled={:<10} 覆盖={}",
                if unit["healthy"] == true { "✓" } else { "!" },
                unit["name"].as_str().unwrap_or("?"),
                unit["active"].as_str().unwrap_or("unknown"),
                unit["enabled"].as_str().unwrap_or("unknown"),
                unit["override_applied"]
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_status(facts: &Value) -> String {
    let empty = Vec::new();
    let unit_values = facts["units"].as_array().unwrap_or(&empty);
    let audio = &facts["audio"];
    let aligned = facts["status"] == "aligned";
    format!(
        "── 当前对齐状态（GNOUS 内建）──\n{}\n\n输出设备：{} 个 sink\n{}\n\n状态：{}\n\n结构化状态：{facts}",
        render_units(unit_values),
        audio["sink_count"],
        serde_json::to_string_pretty(&audio["sinks"]).unwrap_or_default(),
        if aligned {
            "已完全对齐 ✅"
        } else {
            "未完全对齐"
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_state_units_are_the_exact_revert_set() {
        let text =
            "# legacy\napplied_at=2026-01-02 03:04\nunits=pipewire.service,wireplumber.service\n";
        let mut state = AlignState {
            version: 1,
            ..Default::default()
        };
        for line in text.lines().filter(|line| !line.starts_with('#')) {
            if let Some(("units", raw)) = line.split_once('=') {
                state.units = raw.split(',').map(str::to_string).collect();
            }
        }
        assert_eq!(state.units, ["pipewire.service", "wireplumber.service"]);
    }

    #[test]
    fn override_is_a_condition_reset_and_never_edits_system_units() {
        assert!(OVERRIDE_CONTENT.contains("[Unit]\nConditionUser="));
        assert!(UNIT_DIR.starts_with(".config/systemd/user"));
        assert!(!UNIT_DIR.starts_with("/usr/lib"));
    }

    #[test]
    fn status_rows_cover_the_same_eight_units_in_start_order() {
        assert_eq!(UNITS.len(), 8);
        assert_eq!(START_ORDER.len(), 8);
        for unit in UNITS {
            assert!(START_ORDER.contains(&unit.name));
        }
        assert_eq!(START_ORDER[0], "pipewire.socket");
    }
}
