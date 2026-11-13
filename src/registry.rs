//! Compile-time plugin registry.
//!
//! Every extension declares itself in `extensions/<id>/manifest.toml`; the
//! manifest is the single source of truth for its name and for the pages it
//! contributes to the top bar. `docs/EXTENSIONS.md` describes the contract.
//!
//! Registration is static on purpose: an extension is a crate in this
//! workspace, compiled in, so there is no dylib ABI to keep stable. Runtime
//! discovery (GNOME extensions) stays in [`crate::ext`], a separate layer.

use std::sync::OnceLock;

use serde::Deserialize;

/// One `manifest.toml`.
#[derive(Clone, Debug, Deserialize)]
pub struct Manifest {
    pub id: String,
    #[allow(dead_code)]
    pub name: String,
    #[allow(dead_code)]
    pub summary: String,
    #[allow(dead_code)]
    pub version: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub icon: Option<String>,
    pub nav: Option<Nav>,
    #[serde(default)]
    pub views: Vec<View>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Nav {
    #[allow(dead_code)]
    pub label: String,
    /// Section this plugin adds pages to (`overview`, `extensions`, ...).
    #[serde(default = "default_section")]
    pub section: String,
    #[serde(default)]
    pub order: i64,
}

fn default_section() -> String {
    "overview".to_string()
}

#[derive(Clone, Debug, Deserialize)]
pub struct View {
    pub id: String,
    pub label: String,
}

/// Manifests compiled into the binary. Adding a page-level extension means
/// adding its manifest here (and its renderer to `pages/`).
const MANIFESTS: &[(&str, &str)] = &[
    (
        "extensions/processes/manifest.toml",
        include_str!("../extensions/processes/manifest.toml"),
    ),
    (
        "extensions/gnome-fixes/manifest.toml",
        include_str!("../extensions/gnome-fixes/manifest.toml"),
    ),
    (
        "extensions/root-align/manifest.toml",
        include_str!("../extensions/root-align/manifest.toml"),
    ),
    (
        "extensions/startup-layouts/manifest.toml",
        include_str!("../extensions/startup-layouts/manifest.toml"),
    ),
    (
        "extensions/openshell/manifest.toml",
        include_str!("../extensions/openshell/manifest.toml"),
    ),
];

static CACHE: OnceLock<Vec<Manifest>> = OnceLock::new();

/// All built-in plugin manifests, parsed once.
pub fn manifests() -> &'static [Manifest] {
    CACHE.get_or_init(|| {
        let mut all = Vec::with_capacity(MANIFESTS.len());
        for (path, raw) in MANIFESTS {
            match toml::from_str::<Manifest>(raw) {
                Ok(manifest) => all.push(manifest),
                Err(error) => panic!("{path} 解析失败: {error}"),
            }
        }
        all.sort_by_key(|manifest| manifest.nav.as_ref().map_or(0, |nav| nav.order));
        all
    })
}

/// Pages contributed to `section`, in manifest order.
pub fn views_for(section: &str) -> Vec<(&'static Manifest, &'static View)> {
    let mut out = Vec::new();
    for manifest in manifests() {
        let Some(nav) = &manifest.nav else { continue };
        if nav.section != section {
            continue;
        }
        for view in &manifest.views {
            out.push((manifest, view));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifests_parse_and_are_registered() {
        let all = manifests();
        assert!(!all.is_empty());
        let processes = all.iter().find(|m| m.id == "processes").expect("processes");
        assert_eq!(processes.name, "进程");
        assert_eq!(
            processes.nav.as_ref().map(|n| n.section.as_str()),
            Some("overview")
        );
        assert_eq!(processes.views.len(), 5);
        assert!(processes.views.iter().any(|v| v.id == "table"));
        let fixes = all
            .iter()
            .find(|m| m.id == "gnome-fixes")
            .expect("gnome-fixes");
        assert_eq!(
            fixes.nav.as_ref().map(|n| n.section.as_str()),
            Some("extensions")
        );
        assert_eq!(fixes.views.len(), 1);
        let align = all
            .iter()
            .find(|m| m.id == "root-align")
            .expect("root-align");
        assert_eq!(
            align.nav.as_ref().map(|n| n.section.as_str()),
            Some("extensions")
        );
        assert_eq!(align.views.len(), 1);
        assert_eq!(align.views[0].id, "align");
        let startup = all
            .iter()
            .find(|m| m.id == "startup-layouts")
            .expect("startup-layouts");
        assert_eq!(
            startup.nav.as_ref().map(|n| n.section.as_str()),
            Some("startup")
        );
        assert_eq!(
            startup
                .views
                .iter()
                .map(|v| v.id.as_str())
                .collect::<Vec<_>>(),
            vec!["layouts", "boot", "journal", "projects"]
        );
        let shell = all.iter().find(|m| m.id == "openshell").expect("openshell");
        assert_eq!(
            shell.nav.as_ref().map(|n| n.section.as_str()),
            Some("openshell")
        );
        assert_eq!(shell.views.len(), 8);
        assert_eq!(shell.views[0].id, "gateway");
        assert_eq!(shell.views[7].id, "tooling");
    }

    #[test]
    fn views_are_grouped_by_section() {
        let overview = views_for("overview");
        assert_eq!(overview.len(), 5);
        assert!(views_for("appearance").is_empty());
        let extensions = views_for("extensions");
        assert_eq!(extensions.len(), 2);
        assert_eq!(extensions[0].1.id, "fixes");
        assert_eq!(extensions[1].1.id, "align");
        assert_eq!(views_for("startup").len(), 4);
        // The sandbox console owns its own rail section, so its eight views are
        // no longer counted against Extensions.
        let shell = views_for("openshell");
        assert_eq!(shell.len(), 8);
        assert_eq!(shell[0].1.id, "gateway");
        assert_eq!(shell[7].1.id, "tooling");
    }
}
