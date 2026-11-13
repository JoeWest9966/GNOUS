//! Local persistence for the Table view (`~/.config/gnous/table.json`).
//!
//! This file is deliberately separate from the UI state: a watch rule and an
//! operation log must survive a restart, while selection, query and scroll do
//! not. Writes are atomic (temp + rename) so a crash cannot leave half a JSON.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::table_state::TableState;

pub const CONFIG_FILE: &str = "table.json";

/// Everything the Table page wants to remember between launches.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Data {
    pub table: TableState,
    pub log: Vec<Entry>,
}

/// One user-visible operation or interval change. `at` is a real local clock
/// stamp; `ok` is false when the OS refused the action.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub at: String,
    pub kind: String,
    pub detail: String,
    pub ok: bool,
}

impl Entry {
    pub fn new(kind: &str, detail: impl Into<String>, ok: bool) -> Self {
        Self {
            at: stamp(),
            kind: kind.to_string(),
            detail: detail.into(),
            ok,
        }
    }
}

/// Full local timestamp; a journal line must be unambiguous across days.
fn stamp() -> String {
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&now, &mut tm).is_null() {
            return "--".into();
        }
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min,
            tm.tm_sec
        )
    }
}

pub fn path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("gnous").join(CONFIG_FILE)
}

/// Missing or malformed files degrade to defaults; never invent state.
pub fn load() -> Data {
    let raw = match std::fs::read_to_string(path()) {
        Ok(raw) => raw,
        Err(_) => return Data::default(),
    };
    let mut data: Data = serde_json::from_str(&raw).unwrap_or_default();
    data.normalise();
    data
}

impl Data {
    fn normalise(&mut self) {
        self.table.query.clear();
        self.table.paused = false;
        self.table.frozen_pid = None;
        self.table.hot_cpu = self.table.hot_threshold();
        if self.log.len() > 200 {
            let excess = self.log.len() - 200;
            self.log.drain(0..excess);
        }
    }
}

pub fn save(data: &Data) -> Result<(), String> {
    let path = path();
    let display = path.display().to_string();
    let result = (|| -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let raw = serde_json::to_string_pretty(data).map_err(std::io::Error::other)?;
        let temp = path.with_extension("json.tmp");
        std::fs::write(&temp, raw + "\n")?;
        std::fs::rename(temp, &path)
    })();
    result.map_err(|error| format!("写入 {} 失败：{error}", display))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table_state::RefreshRate;

    #[test]
    fn round_trip_keeps_persistent_preferences_only() {
        let mut data = Data::default();
        data.table.refresh = RefreshRate::Slow;
        data.table.query.push_str("discard me");
        data.table.paused = true;
        data.log.push(Entry::new("SIGNAL", "PID 1", true));
        let raw = serde_json::to_string(&data).unwrap();
        let mut restored: Data = serde_json::from_str(&raw).unwrap();
        restored.normalise();
        assert_eq!(restored.table.refresh, RefreshRate::Slow);
        assert_eq!(restored.log.len(), 1);
        assert!(restored.table.query.is_empty());
        assert!(!restored.table.paused);
    }
}
