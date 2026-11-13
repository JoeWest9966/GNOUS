use std::fs::OpenOptions;
use std::io::Write;

use serde_json::{Value, json};

use super::{Outcome, Tool, state};

/// Append-only, machine-readable record of every built-in operation. The log
/// never contains clipboard payloads or `/home/root/GNOME` paths; stdout is
/// summarized by its last line so the audit stays useful and small.
pub fn append(outcome: &Outcome) -> Result<(), String> {
    state::ensure_dir()?;
    let record = json!({
        "ts": crate::sys::local_clock(),
        "tool": outcome.tool.id(),
        "op": outcome.op.id(),
        "ok": outcome.ok,
        "command": outcome.command,
        "summary": summary(&outcome.output),
        "facts": outcome.facts,
    });
    let path = state::audit_file();
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("打开 {} 失败: {e}", path.display()))?;
    let line = serde_json::to_string(&record).map_err(|e| format!("序列化审计记录失败: {e}"))?;
    writeln!(file, "{line}").map_err(|e| format!("写审计记录失败: {e}"))
}

fn summary(output: &str) -> String {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .last()
        .unwrap_or("")
        .chars()
        .take(220)
        .collect()
}

pub fn built_in_tool_ids() -> [&'static str; 2] {
    [Tool::Clipfix.id(), Tool::RootAlign.id()]
}

pub fn no_external_paths(facts: &Value) -> bool {
    let text = facts.to_string();
    !text.contains("/home/root/GNOME") && !text.contains("root-gnome-align.sh")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_keeps_only_the_last_meaningful_line() {
        assert_eq!(summary("第一行\n\n  ✓ 完成  "), "✓ 完成");
        assert_eq!(summary(""), "");
    }

    #[test]
    fn facts_reject_external_tool_paths() {
        assert!(no_external_paths(&json!({"status": "aligned"})));
        assert!(!no_external_paths(
            &json!({"path": "/home/root/GNOME/Root用户桌面功能对齐"})
        ));
    }
}
