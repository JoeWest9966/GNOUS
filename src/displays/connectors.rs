//! `/sys/class/drm` 里的物理连接状态。
//!
//! 这是判断"外接屏插没插"的唯一可靠来源：mutter 在开机早期或刚重配时可能还没
//! 把接口列出来，而 sysfs 永远是内核的实时事实。接口名要和 mutter 的 connector
//! 对齐，所以 `HDMI-A-1` 要归一化成 `HDMI-1`。

use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct Connector {
    pub name: String,
    pub status: String,
}

impl Connector {
    pub fn connected(&self) -> bool {
        self.status == "connected"
    }
}

/// `HDMI-A-1` → `HDMI-1`，`DP-2` 保持 `DP-2`，`eDP-1` 保持 `eDP-1`。
pub fn normalize(raw: &str) -> String {
    if let Some(rest) = raw.strip_prefix("HDMI-A-") {
        return format!("HDMI-{rest}");
    }
    if let Some(rest) = raw.strip_prefix("HDMI-B-") {
        return format!("HDMI-{rest}");
    }
    raw.to_string()
}

/// 扫描 `/sys/class/drm/card*-*/status`，按接口名排序。
pub fn scan(root: &Path) -> Vec<Connector> {
    let mut out: Vec<Connector> = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return out;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        // 只要 "card0-HDMI-A-1" 这种带接口名的条目，跳过 card0 与 card0-DP-1 之外的东西。
        let Some((card, connector)) = name.split_once('-') else {
            continue;
        };
        if !card.starts_with("card") || card.starts_with("card-") {
            continue;
        }
        let status_path = entry.path().join("status");
        let Ok(status) = std::fs::read_to_string(&status_path) else {
            continue;
        };
        let connector = normalize(connector);
        // 同一个接口可能出现在多张卡上（例如核显 + 独显），保留第一个。
        if out.iter().any(|c| c.name == connector) {
            continue;
        }
        out.push(Connector {
            name: connector,
            status: status.trim().to_string(),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub fn status_of(list: &[Connector], name: &str) -> String {
    list.iter()
        .find(|c| c.name == name)
        .map(|c| c.status.clone())
        .unwrap_or_else(|| "absent".to_string())
}

pub fn is_connected(list: &[Connector], name: &str) -> bool {
    list.iter().any(|c| c.name == name && c.connected())
}

pub fn summary(list: &[Connector]) -> String {
    if list.is_empty() {
        return "（没有可用接口）".to_string();
    }
    list.iter()
        .map(|c| format!("{}={}", c.name, c.status))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_tree(root: &Path, entries: &[(&str, &str)]) {
        std::fs::create_dir_all(root).unwrap();
        for (name, status) in entries {
            let dir = root.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("status"), format!("{status}\n")).unwrap();
        }
    }

    #[test]
    fn normalizes_connector_names() {
        assert_eq!(normalize("HDMI-A-1"), "HDMI-1");
        assert_eq!(normalize("HDMI-B-2"), "HDMI-2");
        assert_eq!(normalize("DP-3"), "DP-3");
        assert_eq!(normalize("eDP-1"), "eDP-1");
    }

    #[test]
    fn reads_sysfs_like_this_machine() {
        let dir = std::env::temp_dir().join(format!("gnous-drm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        fake_tree(
            &dir,
            &[
                ("card0-eDP-1", "connected"),
                ("card0-HDMI-A-1", "disconnected"),
                ("card0-DP-1", "disconnected"),
                ("card0", "connected"),
            ],
        );
        let connectors = scan(&dir);
        assert_eq!(
            connectors
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            vec!["DP-1", "HDMI-1", "eDP-1"]
        );
        assert!(is_connected(&connectors, "eDP-1"));
        assert!(!is_connected(&connectors, "HDMI-1"));
        assert_eq!(status_of(&connectors, "HDMI-1"), "disconnected");
        assert_eq!(status_of(&connectors, "VGA-1"), "absent");
        assert!(summary(&connectors).contains("eDP-1=connected"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_root_is_empty_not_a_panic() {
        let missing = std::env::temp_dir().join("gnous-drm-does-not-exist");
        let _ = std::fs::remove_dir_all(&missing);
        assert!(scan(&missing).is_empty());
    }
}
