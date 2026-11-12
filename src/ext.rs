//! Extension inventory and control.
//!
//! GNOME Shell has no plugin API of its own: an extension is a directory with
//! a `metadata.json`, and "enabled" is a string list in gsettings. GNOUS owns
//! this thin provider layer instead of driving `extension-manager` (GUI only,
//! no API) or binding to `org.gnome.Shell` D-Bus (needs the user session bus
//! and ties us to GNOME's rules).
//!
//! [`FilesystemProvider`] is the default backend: it reads drop-in directories
//! directly and toggles the gsettings list, so it works offline and without
//! D-Bus. Future backends (GNOME Shell D-Bus, GNOUS-native extensions) only
//! implement [`Provider`]. `GNOUS_EXTENSIONS_DIR` prepends extra directories,
//! which is how GNOUS-native extensions will be picked up.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

const SHELL_SCHEMA: &str = "org.gnome.shell";
const ENABLED_KEY: &str = "enabled-extensions";

/// One installed extension, as far as the inventory is concerned.
#[derive(Clone, Debug)]
pub struct Extension {
    pub uuid: String,
    pub name: String,
    #[allow(dead_code)]
    pub description: String,
    /// Metadata version, normalized to a string (`79` → `"79"`).
    pub version: String,
    pub enabled: bool,
    pub origin: Origin,
    #[allow(dead_code)]
    pub dir: PathBuf,
    #[allow(dead_code)]
    pub has_prefs: bool,
    #[allow(dead_code)]
    pub shell_versions: Vec<String>,
}

impl Extension {
    /// `v79 · 运行中` / `已禁用` — the second line of an extension row.
    pub fn meta_line(&self) -> String {
        let state = if self.enabled {
            "运行中"
        } else {
            "已禁用"
        };
        if self.version.is_empty() {
            state.to_string()
        } else {
            format!("v{} · {state}", self.version)
        }
    }

    pub fn status(&self) -> (&'static str, bool) {
        if self.enabled {
            ("Active", true)
        } else {
            ("Off", false)
        }
    }
}

/// Where an extension was found. Also the shadowing order: a user copy wins
/// over a system copy with the same UUID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Origin {
    GNous,
    User,
    System,
}

impl Origin {
    /// Where the copy came from, for the row's subtitle.
    pub fn label(self) -> &'static str {
        match self {
            Origin::GNous => "自研",
            Origin::User => "用户",
            Origin::System => "系统",
        }
    }

    pub fn rank(self) -> u8 {
        match self {
            Origin::GNous => 0,
            Origin::User => 1,
            Origin::System => 2,
        }
    }
}

/// A source of extensions. Anything that can list and toggle them qualifies;
/// the UI never talks to a concrete backend.
pub trait Provider {
    #[allow(dead_code)]
    fn id(&self) -> &'static str;
    fn list(&self) -> Vec<Extension>;
    fn set_enabled(&self, uuid: &str, enabled: bool) -> Result<(), String>;
}

// ============================================================================
// Filesystem + gsettings backend
// ============================================================================

pub struct FilesystemProvider {
    dirs: Vec<(Origin, PathBuf)>,
    enabled: Vec<String>,
}

impl FilesystemProvider {
    /// Scan the standard drop-in directories, with the enabled list from
    /// gsettings. Costs one `gsettings get`, so only do this on startup and on
    /// a slow tick.
    pub fn discover() -> Self {
        Self {
            dirs: discover_dirs(),
            enabled: enabled_list(),
        }
    }

    #[allow(dead_code)]
    pub fn with_dirs(dirs: Vec<(Origin, PathBuf)>) -> Self {
        Self {
            dirs,
            enabled: Vec::new(),
        }
    }

    /// Re-read the enabled list after something else may have changed it.
    pub fn reload(&mut self) {
        self.enabled = enabled_list();
    }

    pub fn entries(&self) -> Vec<Extension> {
        let mut out: Vec<Extension> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for (origin, dir) in &self.dirs {
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            let mut found: Vec<Extension> = entries
                .flatten()
                .filter(|e| e.path().is_dir())
                .filter_map(|e| read_metadata(&e.path(), *origin, &self.enabled))
                .collect();
            found.sort_by_cached_key(|e| e.name.to_lowercase());
            for ext in found {
                if seen.insert(ext.uuid.clone()) {
                    out.push(ext);
                }
            }
        }
        out.sort_by_cached_key(|e| (e.origin.rank(), e.name.to_lowercase()));
        out
    }

    pub fn toggle(&self, uuid: &str, enabled: bool) -> Result<(), String> {
        let mut list = enabled_list();
        apply_enabled(&mut list, uuid, enabled);
        write_enabled(&list)
    }
}

impl Provider for FilesystemProvider {
    fn id(&self) -> &'static str {
        "filesystem"
    }

    fn list(&self) -> Vec<Extension> {
        self.entries()
    }

    fn set_enabled(&self, uuid: &str, enabled: bool) -> Result<(), String> {
        self.toggle(uuid, enabled)
    }
}

// ============================================================================
// metadata.json / gsettings plumbing
// ============================================================================

#[derive(Deserialize)]
struct Metadata {
    uuid: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    /// Some extensions ship a number, some a string; accept both.
    #[serde(default)]
    version: Option<serde_json::Value>,
    #[serde(default, rename = "settings-schema")]
    settings_schema: Option<String>,
    #[serde(default, rename = "shell-version")]
    shell_version: Vec<String>,
}

fn read_metadata(dir: &Path, origin: Origin, enabled: &[String]) -> Option<Extension> {
    let raw = std::fs::read_to_string(dir.join("metadata.json")).ok()?;
    let meta: Metadata = serde_json::from_str(&raw).ok()?;
    let uuid = meta.uuid.trim().to_string();
    if uuid.is_empty() {
        return None;
    }
    let version = match meta.version {
        Some(serde_json::Value::String(s)) => s,
        Some(serde_json::Value::Number(n)) => n.to_string(),
        _ => String::new(),
    };
    Some(Extension {
        name: meta.name.unwrap_or_else(|| uuid.clone()),
        description: meta.description.unwrap_or_default(),
        has_prefs: meta.settings_schema.is_some() || dir.join("prefs.js").exists(),
        shell_versions: meta.shell_version,
        enabled: enabled.iter().any(|e| e == &uuid),
        origin,
        dir: dir.to_path_buf(),
        uuid,
        version,
    })
}

/// `GNOUS_EXTENSIONS_DIR`, then `$XDG_DATA_HOME`, then `$XDG_DATA_DIRS`, each
/// with `/gnome-shell/extensions` appended.
pub fn discover_dirs() -> Vec<(Origin, PathBuf)> {
    let mut dirs = Vec::new();
    if let Some(raw) = std::env::var_os("GNOUS_EXTENSIONS_DIR") {
        for path in std::env::split_paths(&raw) {
            dirs.push((Origin::GNous, path));
        }
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| home.map(|h| h.join(".local/share")));
    if let Some(dir) = data_home {
        dirs.push((Origin::User, dir.join("gnome-shell/extensions")));
    }
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string());
    for dir in data_dirs.split(':').filter(|d| !d.is_empty()) {
        dirs.push((
            Origin::System,
            Path::new(dir).join("gnome-shell/extensions"),
        ));
    }
    dirs
}

fn gsettings(args: &[&str]) -> Option<String> {
    let out = Command::new("gsettings").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn enabled_list() -> Vec<String> {
    gsettings(&["get", SHELL_SCHEMA, ENABLED_KEY])
        .map(|raw| parse_strv(&raw))
        .unwrap_or_default()
}

/// Set or clear `uuid` in an enabled list, in place.
fn apply_enabled(list: &mut Vec<String>, uuid: &str, enabled: bool) {
    let position = list.iter().position(|e| e == uuid);
    match (enabled, position) {
        (true, None) => list.push(uuid.to_string()),
        (false, Some(index)) => {
            list.remove(index);
        }
        _ => {}
    }
}

fn write_enabled(list: &[String]) -> Result<(), String> {
    let items: Vec<String> = list
        .iter()
        .map(|item| format!("'{}'", item.replace('\\', "\\\\").replace('\'', "\\'")))
        .collect();
    let value = format!("[{}]", items.join(", "));
    let out = Command::new("gsettings")
        .args(["set", SHELL_SCHEMA, ENABLED_KEY, &value])
        .output()
        .map_err(|error| format!("无法执行 gsettings: {error}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(())
}

/// Parse a GVariant string array (`['a', 'b']`, `@as []`, `@as ['a']`).
fn parse_strv(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\'' {
            continue;
        }
        let mut item = String::new();
        while let Some(next) = chars.next() {
            match next {
                '\\' => {
                    if let Some(escaped) = chars.next() {
                        item.push(escaped);
                    }
                }
                '\'' => break,
                other => item.push(other),
            }
        }
        out.push(item);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gnous-ext-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn write_extension(root: &Path, uuid: &str, metadata: &str) {
        let dir = root.join(uuid);
        std::fs::create_dir_all(&dir).expect("extension dir");
        std::fs::write(dir.join("metadata.json"), metadata).expect("metadata");
    }

    #[test]
    fn parses_gvariant_lists() {
        assert!(parse_strv("@as []").is_empty());
        assert_eq!(parse_strv("['a', 'b']"), vec!["a", "b"]);
        assert_eq!(parse_strv("@as ['a@b', 'c']"), vec!["a@b", "c"]);
        assert_eq!(parse_strv("['it\\'s']"), vec!["it's"]);
    }

    #[test]
    fn toggles_enabled_list() {
        let mut list = vec!["a".to_string()];
        apply_enabled(&mut list, "b", true);
        assert_eq!(list, vec!["a", "b"]);
        apply_enabled(&mut list, "a", false);
        assert_eq!(list, vec!["b"]);
        apply_enabled(&mut list, "b", false);
        assert!(list.is_empty());
        apply_enabled(&mut list, "b", false);
        assert!(list.is_empty());
    }

    #[test]
    fn reads_metadata_and_shadows_system_copies() {
        let user = temp_dir("user");
        let system = temp_dir("system");
        write_extension(
            &user,
            "demo@gnous",
            r#"{"uuid":"demo@gnous","name":"Demo","description":"d","version":3,"shell-version":["48"]}"#,
        );
        write_extension(
            &system,
            "demo@gnous",
            r#"{"uuid":"demo@gnous","name":"Demo (system)","version":"9"}"#,
        );
        write_extension(
            &system,
            "other@gnous",
            r#"{"uuid":"other@gnous","name":"Other"}"#,
        );

        let provider = FilesystemProvider {
            dirs: vec![
                (Origin::User, user.clone()),
                (Origin::System, system.clone()),
            ],
            enabled: vec!["demo@gnous".to_string()],
        };
        let entries = provider.entries();
        assert_eq!(entries.len(), 2, "{entries:#?}");
        let demo = entries
            .iter()
            .find(|e| e.uuid == "demo@gnous")
            .expect("demo");
        assert_eq!(demo.name, "Demo");
        assert_eq!(demo.version, "3");
        assert!(demo.enabled);
        assert_eq!(demo.origin, Origin::User);
        assert_eq!(demo.meta_line(), "v3 · 运行中");
        let other = entries
            .iter()
            .find(|e| e.uuid == "other@gnous")
            .expect("other");
        assert!(!other.enabled);
        assert_eq!(other.origin, Origin::System);

        let _ = std::fs::remove_dir_all(user);
        let _ = std::fs::remove_dir_all(system);
    }

    #[test]
    fn ignores_broken_metadata() {
        let root = temp_dir("broken");
        write_extension(&root, "bad@gnous", "{ not json");
        write_extension(&root, "empty@gnous", r#"{"name":"no uuid"}"#);
        let provider = FilesystemProvider::with_dirs(vec![(Origin::User, root.clone())]);
        assert!(provider.entries().is_empty());
        let _ = std::fs::remove_dir_all(root);
    }
}
