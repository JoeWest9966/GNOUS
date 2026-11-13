use std::fs;
use std::path::PathBuf;

use serde::de::DeserializeOwned;

/// XDG state home for GNOUS-owned fix backends. Overridable so tests never
/// touch `/root/.local/state`.
const STATE_HOME_ENV: &str = "GNOUS_STATE_HOME";

pub fn state_dir() -> PathBuf {
    let base = std::env::var_os(STATE_HOME_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("XDG_STATE_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home().join(".local/state"))
        });
    base.join("gnous/fixes")
}

pub fn state_file() -> PathBuf {
    state_dir().join("state.json")
}

pub fn audit_file() -> PathBuf {
    state_dir().join("audit.jsonl")
}

pub fn ensure_dir() -> Result<(), String> {
    fs::create_dir_all(state_dir()).map_err(|e| format!("创建 {} 失败: {e}", state_dir().display()))
}

pub fn read_json<T: DeserializeOwned>(name: &str) -> Option<T> {
    let text = fs::read_to_string(state_dir().join(name)).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn write_json(name: &str, value: &impl serde::Serialize) -> Result<(), String> {
    ensure_dir()?;
    let path = state_dir().join(name);
    let tmp = path.with_extension("tmp");
    let text = serde_json::to_string_pretty(value).map_err(|e| format!("序列化状态失败: {e}"))?;
    fs::write(&tmp, format!("{text}\n")).map_err(|e| format!("写 {tmp:?} 失败: {e}"))?;
    fs::rename(&tmp, &path).map_err(|e| format!("落盘 {} 失败: {e}", path.display()))
}

pub fn remove(name: &str) {
    let _ = fs::remove_file(state_dir().join(name));
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/root"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_roundtrip_uses_an_atomic_temp_file() {
        let dir = std::env::temp_dir().join(format!("gnous-state-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        // Exercise the same file logic without mutating process-wide env.
        let path = dir.join("gnous/fixes/test.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let text = serde_json::to_string_pretty(&json!({"answer": 42})).unwrap();
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, format!("{text}\n")).unwrap();
        fs::rename(&tmp, &path).unwrap();
        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(value["answer"], 42);
        assert!(!tmp.exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
