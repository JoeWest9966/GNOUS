//! 多屏幕扩展的配置：`$XDG_CONFIG_HOME/gnous/displays.json`。
//!
//! 缺省值刻意等于"本机现在的行为"（`auto` + 2 秒对账），这样装上 GNOUS 不会
//! 悄悄改变任何东西；要改行为必须显式写一条配置或点一次应用。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// 外接屏接了且 mutter 认它 → 只开外接屏，否则只开内置屏。
    Auto,
    /// 只开外接屏。
    External,
    /// 只开内置屏。
    Internal,
    /// 两块都开（扩展，内置屏在左）。
    Both,
}

impl Mode {
    pub const ALL: [Mode; 4] = [Mode::Auto, Mode::External, Mode::Internal, Mode::Both];

    pub fn id(self) -> &'static str {
        match self {
            Mode::Auto => "auto",
            Mode::External => "external",
            Mode::Internal => "internal",
            Mode::Both => "both",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::Auto => "AUTO",
            Mode::External => "外接",
            Mode::Internal => "内置",
            Mode::Both => "双屏",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Mode::Auto => "有外接屏用外接屏，拔掉回内置屏",
            Mode::External => "只开外接屏",
            Mode::Internal => "只开内置屏",
            Mode::Both => "两块都开，扩展排列",
        }
    }

    pub fn parse(text: &str) -> Option<Mode> {
        Mode::ALL.into_iter().find(|m| m.id() == text)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Resolution {
    /// mutter 标的 `is-preferred`。
    Preferred,
    /// 分辨率最高的模式。
    Max,
    /// 当前正在用的模式（没有就退回 preferred）。
    Current,
}

impl Resolution {
    pub const ALL: [Resolution; 3] = [Resolution::Preferred, Resolution::Max, Resolution::Current];

    pub fn id(self) -> &'static str {
        match self {
            Resolution::Preferred => "preferred",
            Resolution::Max => "max",
            Resolution::Current => "current",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Resolution::Preferred => "首选",
            Resolution::Max => "最高",
            Resolution::Current => "当前",
        }
    }

    pub fn parse(text: &str) -> Option<Resolution> {
        Resolution::ALL.into_iter().find(|r| r.id() == text)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    pub mode: Mode,
    /// 外接屏的 connector 名（mutter 的写法，例如 `HDMI-1`）。
    pub external: String,
    /// 内置屏的 connector 名。
    pub internal: String,
    pub resolution: Resolution,
    /// GNOUS 自己接管时的对账周期，秒。1..=60。
    pub reconcile_secs: u64,
    /// 交棒之后置真：表示"这台机器的显示已经由 GNOUS 管"。
    pub takeover: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            mode: Mode::Auto,
            external: "HDMI-1".to_string(),
            internal: "eDP-1".to_string(),
            resolution: Resolution::Preferred,
            reconcile_secs: 2,
            takeover: false,
        }
    }
}

impl Config {
    pub fn targets(&self, external_usable: bool) -> Vec<String> {
        match self.mode {
            Mode::Auto => {
                if external_usable {
                    vec![self.external.clone()]
                } else {
                    vec![self.internal.clone()]
                }
            }
            Mode::External => vec![self.external.clone()],
            Mode::Internal => vec![self.internal.clone()],
            // 内置屏在左，外接屏在右，和 display-mode 的老行为一致。
            Mode::Both => vec![self.internal.clone(), self.external.clone()],
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if !(1..=60).contains(&self.reconcile_secs) {
            return Err(format!(
                "对账周期 {} 秒超出范围（1..=60）",
                self.reconcile_secs
            ));
        }
        if self.external.trim().is_empty() || self.internal.trim().is_empty() {
            return Err("外接屏与内置屏的 connector 名都不能为空".to_string());
        }
        if self.external == self.internal {
            return Err(format!(
                "外接屏与内置屏不能是同一个接口（{}）",
                self.external
            ));
        }
        Ok(())
    }

    pub fn summary(&self) -> String {
        format!(
            "{} · {}→{} · 分辨率 {} · 对账 {}s",
            self.mode.id(),
            self.internal,
            self.external,
            self.resolution.id(),
            self.reconcile_secs
        )
    }
}

pub fn config_path() -> PathBuf {
    if let Some(path) = std::env::var_os("GNOUS_DISPLAY_CONFIG") {
        return PathBuf::from(path);
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"));
    base.join("gnous/displays.json")
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/root"), PathBuf::from)
}

/// 读配置。文件不存在是正常情况（返回默认值），格式坏了才会报错。
pub fn load() -> (Config, Option<String>) {
    let path = config_path();
    let Ok(text) = std::fs::read_to_string(&path) else {
        return (Config::default(), None);
    };
    match serde_json::from_str::<Config>(&text) {
        Ok(config) => match config.validate() {
            Ok(()) => (config, None),
            Err(why) => (
                Config::default(),
                Some(format!("{} 不合法：{why}（按默认值显示）", path.display())),
            ),
        },
        Err(error) => (
            Config::default(),
            Some(format!(
                "{} 解析失败：{error}（按默认值显示）",
                path.display()
            )),
        ),
    }
}

/// 原子写：临时文件 + rename，避免半个 JSON。
pub fn save(config: &Config) -> Result<PathBuf, String> {
    config.validate()?;
    let path = config_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建 {} 失败：{e}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(config).map_err(|e| format!("序列化失败：{e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, format!("{text}\n"))
        .map_err(|e| format!("写 {} 失败：{e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("替换 {} 失败：{e}", path.display()))?;
    Ok(path)
}

/// 图表/文案要用到的"是否 dry-run"开关。
pub fn dry_run() -> bool {
    matches!(
        std::env::var("GNOUS_DISPLAY_DRY_RUN").as_deref(),
        Ok("1") | Ok("true")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 环境变量是进程级的，并行测试必须串行化，否则会互相踩。
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn defaults_match_the_old_script() {
        let config = Config::default();
        assert_eq!(config.mode, Mode::Auto);
        assert_eq!(config.external, "HDMI-1");
        assert_eq!(config.internal, "eDP-1");
        assert_eq!(config.reconcile_secs, 2);
        assert!(config.validate().is_ok());
        assert_eq!(
            config.targets(true),
            vec!["HDMI-1".to_string()],
            "外接可用时只开外接"
        );
        assert_eq!(
            config.targets(false),
            vec!["eDP-1".to_string()],
            "外接不可用时回内置"
        );
    }

    #[test]
    fn both_puts_internal_first() {
        let config = Config {
            mode: Mode::Both,
            ..Config::default()
        };
        assert_eq!(
            config.targets(true),
            vec!["eDP-1".to_string(), "HDMI-1".to_string()]
        );
    }

    #[test]
    fn rejects_bad_values() {
        let mut config = Config::default();
        config.reconcile_secs = 0;
        assert!(config.validate().is_err());
        config.reconcile_secs = 61;
        assert!(config.validate().is_err());
        config.reconcile_secs = 2;
        config.external = String::new();
        assert!(config.validate().is_err());
        config.external = "eDP-1".into();
        assert!(config.validate().is_err(), "两个接口不能相同");
    }

    #[test]
    fn roundtrips_through_disk() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("gnous-display-config-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("displays.json");
        unsafe { std::env::set_var("GNOUS_DISPLAY_CONFIG", &path) };
        let config = Config {
            mode: Mode::Both,
            resolution: Resolution::Max,
            reconcile_secs: 5,
            takeover: true,
            ..Config::default()
        };
        let written = save(&config).expect("应该能写");
        assert_eq!(written, path);
        let (loaded, warning) = load();
        assert!(warning.is_none(), "{warning:?}");
        assert_eq!(loaded, config);
        unsafe { std::env::remove_var("GNOUS_DISPLAY_CONFIG") };
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn broken_json_falls_back_with_a_reason() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("gnous-display-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("displays.json");
        std::fs::write(&path, "{ not json").unwrap();
        unsafe { std::env::set_var("GNOUS_DISPLAY_CONFIG", &path) };
        let (config, warning) = load();
        assert_eq!(config, Config::default());
        assert!(warning.unwrap_or_default().contains("解析失败"));
        unsafe { std::env::remove_var("GNOUS_DISPLAY_CONFIG") };
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn every_mode_id_roundtrips() {
        for mode in Mode::ALL {
            assert_eq!(Mode::parse(mode.id()), Some(mode));
        }
        for resolution in Resolution::ALL {
            assert_eq!(Resolution::parse(resolution.id()), Some(resolution));
        }
        assert_eq!(Mode::parse("nope"), None);
    }
}
