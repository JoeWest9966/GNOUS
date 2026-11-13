//! systemd 用户单元：读取老脚本的两只单元，写 GNOUS 自己的两只单元。
//!
//! 边界和老脚本那套一样清楚：
//! - `display-mode.timer` / `display-mode.service` 是**别人**的，只读；
//! - `gnous-display.timer` / `gnous-display.service` 是 GNOUS 的，可以写、删；
//! - 启用/停用谁，由人手执行 `handover` 打印出来的命令，GNOUS 不代劳。

use std::collections::BTreeMap;
use std::path::PathBuf;

use super::config::Config;
use super::exec;

pub const LEGACY_SERVICE: &str = "display-mode.service";
pub const LEGACY_TIMER: &str = "display-mode.timer";
pub const OWN_SERVICE: &str = "gnous-display.service";
pub const OWN_TIMER: &str = "gnous-display.timer";

const PROPS: &str =
    "LoadState,ActiveState,SubState,UnitFileState,ExecStart,FragmentPath,ExecMainPID";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct UnitInfo {
    pub unit: String,
    pub properties: BTreeMap<String, String>,
    pub error: Option<String>,
}

impl UnitInfo {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.properties.get(key).map(String::as_str)
    }

    pub fn present(&self) -> bool {
        !matches!(self.get("LoadState"), Some("not-found") | None)
    }

    pub fn active(&self) -> bool {
        matches!(
            self.get("ActiveState"),
            Some("active" | "activating" | "reloading")
        )
    }

    pub fn enabled(&self) -> bool {
        matches!(
            self.get("UnitFileState"),
            Some("enabled" | "enabled-runtime")
        )
    }

    pub fn exec(&self) -> String {
        self.get("ExecStart").map(str::to_owned).unwrap_or_default()
    }

    pub fn one_line(&self) -> String {
        if let Some(error) = &self.error {
            return format!("{} 查询失败：{error}", self.unit);
        }
        if !self.present() {
            return format!("{} 未安装", self.unit);
        }
        format!(
            "{} {} / {}{}",
            self.unit,
            self.get("ActiveState").unwrap_or("?"),
            self.get("SubState").unwrap_or("?"),
            self.get("UnitFileState")
                .map(|s| format!(" ({s})"))
                .unwrap_or_default()
        )
    }
}

pub fn info(unit: &str) -> UnitInfo {
    let mut info = UnitInfo {
        unit: unit.to_string(),
        ..UnitInfo::default()
    };
    match exec::run(&["systemctl", "--user", "show", unit, "-p", PROPS]) {
        Ok(text) => {
            for line in text.lines() {
                if let Some((key, value)) = line.split_once('=') {
                    info.properties
                        .insert(key.trim().to_string(), value.trim().to_string());
                }
            }
            if info.properties.is_empty() {
                info.error = Some("systemctl show 没有任何输出".into());
            }
        }
        Err(error) => info.error = Some(error),
    }
    info
}

pub fn unit_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("GNOUS_DISPLAY_UNIT_DIR") {
        return PathBuf::from(dir);
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"));
    base.join("systemd/user")
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/root"), PathBuf::from)
}

pub fn log_file() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".cache"));
    base.join("gnous/displays.log")
}

pub fn service_file() -> PathBuf {
    unit_dir().join(OWN_SERVICE)
}

pub fn timer_file() -> PathBuf {
    unit_dir().join(OWN_TIMER)
}

/// 服务单元：一次只做一件事——把当前配置对账一遍，然后退出。
/// 不开 `Restart=`，因为它本来就是被 timer 每秒/每几秒叫起来的短命进程。
pub fn render_service(config: &Config, exe: &std::path::Path) -> String {
    format!(
        "\
[Unit]
Description=GNOUS 多屏幕对账（{}）
Documentation=https://github.com/JoeWest9966/GNOUS

[Service]
Type=oneshot
ExecStart={exe} --builtin-display-auto
StandardOutput=append:{log}
StandardError=append:{log}
",
        config.summary(),
        exe = exe.display(),
        log = log_file().display(),
    )
}

pub fn render_timer(config: &Config) -> String {
    format!(
        "\
[Unit]
Description=GNOUS 多屏幕对账定时器（每 {secs}s）

[Timer]
OnBootSec=5s
OnUnitActiveSec={secs}s
AccuracySec=200ms

[Install]
WantedBy=timers.target
",
        secs = config.reconcile_secs
    )
}

/// 写两只单元文件。**不 enable、不 start、不 daemon-reload**。
pub fn install(config: &Config, exe: &std::path::Path) -> Result<Vec<String>, String> {
    config.validate()?;
    if super::config::dry_run() {
        return Ok(vec![
            "[dry-run] 会写 ".to_string() + &service_file().display().to_string(),
            "[dry-run] 会写 ".to_string() + &timer_file().display().to_string(),
            "[dry-run] 不会 enable / start 任何单元".to_string(),
        ]);
    }
    let dir = unit_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建 {} 失败：{e}", dir.display()))?;
    let mut steps = Vec::new();
    for (path, body) in [
        (service_file(), render_service(config, exe)),
        (timer_file(), render_timer(config)),
    ] {
        std::fs::write(&path, body).map_err(|e| format!("写 {} 失败：{e}", path.display()))?;
        steps.push(format!("已写 {}", path.display()));
    }
    if let Some(log) = log_file().parent() {
        let _ = std::fs::create_dir_all(log);
    }
    steps.push("未 enable / 未 start：交棒要由人手执行 HANDOVER 里的命令".to_string());
    Ok(steps)
}

/// 只删 GNOUS 自己的两只文件。
pub fn remove() -> Result<Vec<String>, String> {
    let mut steps = Vec::new();
    for path in [timer_file(), service_file()] {
        match std::fs::remove_file(&path) {
            Ok(()) => steps.push(format!("已删 {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                steps.push(format!("{} 本来就不存在", path.display()))
            }
            Err(e) => return Err(format!("删 {} 失败：{e}", path.display())),
        }
    }
    steps.push(format!("{} 未被触碰（GNOUS 从不改它）", LEGACY_TIMER));
    Ok(steps)
}

/// 人手执行的交棒命令：把显示控制权从老脚本交给 GNOUS。
/// 只打印，永远不执行——顺序错了会把屏幕弄黑，得由看着屏幕的人决定时机。
pub fn handover_commands(config: &Config) -> Vec<String> {
    vec![
        format!("systemctl --user stop {LEGACY_TIMER} {LEGACY_SERVICE}"),
        format!("systemctl --user disable {LEGACY_TIMER} {LEGACY_SERVICE}"),
        "systemctl --user daemon-reload".to_string(),
        "systemctl --user enable --now gnous-display.timer".to_string(),
        "systemctl --user list-timers gnous-display.timer --no-pager".to_string(),
        // 后悔药
        format!(
            "# 后悔药：systemctl --user disable --now gnous-display.timer && systemctl --user enable --now {LEGACY_TIMER}"
        ),
        format!("# 当前配置：{}", config.summary()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::displays::config::{Mode, Resolution};

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn config() -> Config {
        Config {
            mode: Mode::Auto,
            resolution: Resolution::Preferred,
            reconcile_secs: 3,
            ..Config::default()
        }
    }

    #[test]
    fn service_runs_our_own_binary_not_the_python_script() {
        let exe = std::path::Path::new("/usr/local/bin/gnous");
        let text = render_service(&config(), exe);
        assert!(text.contains("ExecStart=/usr/local/bin/gnous --builtin-display-auto"));
        assert!(!text.contains("python3"), "{text}");
        assert!(!text.contains("/home/root/GNOME"), "{text}");
        assert!(text.contains("Type=oneshot"));
        assert!(!text.contains("Restart="), "oneshot 不需要 Restart");
    }

    #[test]
    fn timer_uses_the_configured_period() {
        let text = render_timer(&config());
        assert!(text.contains("OnUnitActiveSec=3s"));
        assert!(text.contains("WantedBy=timers.target"));
    }

    #[test]
    fn install_and_remove_only_touch_our_files() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("gnous-display-units-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        unsafe { std::env::set_var("GNOUS_DISPLAY_UNIT_DIR", &dir) };
        unsafe { std::env::remove_var("GNOUS_DISPLAY_DRY_RUN") };

        let steps = install(&config(), std::path::Path::new("/usr/local/bin/gnous")).unwrap();
        assert!(service_file().exists());
        assert!(timer_file().exists());
        assert!(steps.iter().any(|s| s.contains("未 enable")));

        let removed = remove().unwrap();
        assert!(!service_file().exists());
        assert!(!timer_file().exists());
        assert!(
            removed.iter().any(|s| s.contains("display-mode.timer")),
            "删除时必须说明老单元没被动过"
        );

        unsafe { std::env::remove_var("GNOUS_DISPLAY_UNIT_DIR") };
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dry_run_writes_nothing() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("gnous-display-dry-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        unsafe { std::env::set_var("GNOUS_DISPLAY_UNIT_DIR", &dir) };
        unsafe { std::env::set_var("GNOUS_DISPLAY_DRY_RUN", "1") };
        let steps = install(&config(), std::path::Path::new("/usr/local/bin/gnous")).unwrap();
        assert!(!dir.exists());
        assert!(steps.iter().all(|s| s.starts_with("[dry-run]")));
        unsafe { std::env::remove_var("GNOUS_DISPLAY_DRY_RUN") };
        unsafe { std::env::remove_var("GNOUS_DISPLAY_UNIT_DIR") };
    }

    #[test]
    fn handover_prints_rollback_and_never_runs_it() {
        let commands = handover_commands(&config());
        assert!(commands[0].contains("stop display-mode.timer"));
        assert!(commands[3].contains("enable --now gnous-display.timer"));
        assert!(
            commands.iter().any(|c| c.contains("后悔药")),
            "必须给出回旧脚本的方法"
        );
    }

    #[test]
    fn unit_information_parses_systemctl_output() {
        let mut info = UnitInfo {
            unit: LEGACY_TIMER.into(),
            ..UnitInfo::default()
        };
        info.properties.insert("LoadState".into(), "loaded".into());
        info.properties
            .insert("ActiveState".into(), "active".into());
        info.properties
            .insert("UnitFileState".into(), "enabled".into());
        assert!(info.present() && info.active() && info.enabled());
        assert!(info.one_line().contains("active"));

        let missing = UnitInfo {
            unit: "x.service".into(),
            properties: [("LoadState".to_string(), "not-found".to_string())]
                .into_iter()
                .collect(),
            ..UnitInfo::default()
        };
        assert!(!missing.present());
    }
}
