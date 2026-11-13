//! The systemd user unit that runs GNOUS's own relay.
//!
//! GNOUS only ever writes, enables, starts, stops or removes **this** unit. The
//! legacy `j1900-proxy.service` stays exactly as the operator left it; handing
//! the live port over is a separate, explicitly-confirmed operation that prints
//! commands instead of running them.

use std::path::PathBuf;

use super::config::{self, Config};
use super::guard;

pub const UNIT_NAME: &str = guard::OWN_UNIT;

pub fn unit_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"));
    base.join("systemd/user")
}

pub fn unit_file() -> PathBuf {
    unit_dir().join(UNIT_NAME)
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/root"), PathBuf::from)
}

pub fn log_file() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".cache"));
    base.join("gnous/proxy-relay.log")
}

/// Render the unit. `exe` is the GNOUS binary itself, so the relay always runs
/// the same build as the panel.
pub fn render(config: &Config, exe: &std::path::Path) -> String {
    let allow_override = if config.adopted {
        // Only the deliberately adopted configuration may bind the live relay
        // port; without this flag the relay refuses to start (guard::check_port).
        format!("Environment={}=1\n", guard::ALLOW_PROTECTED_ENV)
    } else {
        String::new()
    };
    format!(
        "\
[Unit]
Description=GNOUS proxy relay ({listen} -> {upstream})
Documentation=https://github.com/JoeWest9966/GNOUS
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart={exe} --builtin-proxy-relay --listen {listen} --upstream {upstream} --idle-timeout {idle} --connect-timeout {connect} --retries {retries}
Restart=always
RestartSec=3
{allow_override}StandardOutput=append:{log}
StandardError=append:{log}

[Install]
WantedBy=default.target
",
        listen = config.listen,
        upstream = config.upstream,
        exe = exe.display(),
        idle = config.idle_timeout_secs,
        connect = config.connect_timeout_secs,
        retries = config.retries,
        allow_override = allow_override,
        log = log_file().display(),
    )
}

/// Write config + unit file. Never touches systemd: enabling is a separate,
/// visible step so a half-written unit can never start on its own.
pub fn install(config: &Config, exe: &std::path::Path) -> Result<Vec<String>, String> {
    let mut steps = Vec::new();
    let unit_text = render(config, exe);
    if guard::dry_run() {
        steps.push(format!(
            "[dry-run] 将写入 {}",
            config::config_file().display()
        ));
        steps.push(format!("[dry-run] 将写入 {}", unit_file().display()));
        steps.push(format!("[dry-run] 单元内容：\n{unit_text}"));
        return Ok(steps);
    }
    let config_path = config::save(config)?;
    steps.push(format!("写入 {}", config_path.display()));
    let dir = unit_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建 {} 失败: {e}", dir.display()))?;
    let path = unit_file();
    let tmp = path.with_extension("service.tmp");
    std::fs::write(&tmp, &unit_text).map_err(|e| format!("写 {} 失败: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("落盘 {} 失败: {e}", path.display()))?;
    steps.push(format!("写入 {}", path.display()));
    steps.push("未启用/未启动（需要时由你点「启用」）".into());
    Ok(steps)
}

/// Remove our unit file (and, when running, stop it first). Legacy untouched.
pub fn remove(installed: bool) -> Result<Vec<String>, String> {
    let mut steps = Vec::new();
    if installed {
        if guard::dry_run() {
            steps.push(format!("[dry-run] systemctl --user stop {UNIT_NAME}")); // safety-allow: 只打印，目标是 GNOUS 自己的单元
        } else {
            let _ = super::exec::run(&["systemctl", "--user", "stop", UNIT_NAME]);
            steps.push(format!("已停止 {UNIT_NAME}"));
        }
    }
    let path = unit_file();
    if path.exists() {
        if guard::dry_run() {
            steps.push(format!("[dry-run] 删除 {}", path.display()));
        } else {
            std::fs::remove_file(&path)
                .map_err(|e| format!("删除 {} 失败: {e}", path.display()))?;
            steps.push(format!("删除 {}", path.display()));
        }
    } else {
        steps.push(format!("{} 不存在，跳过", path.display()));
    }
    Ok(steps)
}

/// Commands the operator pastes to hand the live port over to GNOUS. GNOUS
/// never runs these itself: they interrupt every in-flight connection,
/// including the one carrying this session.
pub fn handover_commands(config: &Config) -> Vec<String> {
    vec![
        "# 1. 先确认 GNOUS 自己的中继能在别的端口跑通（不影响现有代理）".to_string(),
        format!(
            "GNOUS_PROXY_ALLOW_PROTECTED_PORT=1 {} --builtin-proxy-relay --listen 127.0.0.1:31299 --upstream {} --idle-timeout {}",
            "gnous", config.upstream, config.idle_timeout_secs
        ),
        "# 2. 交棒：停掉旧中继、接管端口（这一瞬间所有连接会断，请挑个空闲时刻）".to_string(),
        "systemctl --user stop j1900-proxy.service".to_string(), // safety-allow: 打印给操作员的交棒脚本，GNOUS 不执行
        "systemctl --user disable j1900-proxy.service".to_string(), // safety-allow: 同上
        "systemctl --user daemon-reload".to_string(),            // safety-allow: 同上
        format!("systemctl --user enable --now {UNIT_NAME}"),
        "# 3. 验收：端口应指向 GNOUS 的中继，控制面无需改动".to_string(),
        format!("systemctl --user status {UNIT_NAME} --no-pager"),
        format!("ss -ltnp | grep :{}", port_of(&config.listen)),
        "# 4. 后悔药：回到旧中继".to_string(),
        format!("systemctl --user disable --now {UNIT_NAME}"),
        format!("systemctl --user enable --now j1900-proxy.service"),
    ]
}

pub fn port_of(addr: &str) -> String {
    addr.rsplit_once(':')
        .map(|(_, port)| port.to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example() -> Config {
        Config::default()
    }

    #[test]
    fn rendered_unit_runs_the_gnous_binary_and_never_the_legacy_script() {
        let text = render(&example(), std::path::Path::new("/usr/local/bin/gnous"));
        assert!(text.contains("ExecStart=/usr/local/bin/gnous --builtin-proxy-relay"));
        assert!(text.contains("Restart=always"));
        assert!(!text.contains("jproxy"));
        assert!(!text.contains("/home/root/GNOME"));
        assert!(!text.contains("python3"));
    }

    #[test]
    fn the_live_port_override_is_only_present_once_adopted() {
        let plain = render(&example(), std::path::Path::new("/usr/local/bin/gnous"));
        assert!(!plain.contains(guard::ALLOW_PROTECTED_ENV));
        let adopted = Config {
            adopted: true,
            ..example()
        };
        let text = render(&adopted, std::path::Path::new("/usr/local/bin/gnous"));
        assert!(text.contains(&format!("Environment={}=1", guard::ALLOW_PROTECTED_ENV)));
    }

    #[test]
    fn unit_name_is_the_guarded_own_unit() {
        assert_eq!(UNIT_NAME, guard::OWN_UNIT);
        assert!(unit_file().to_string_lossy().ends_with(guard::OWN_UNIT));
    }

    #[test]
    fn handover_commands_are_printed_not_executed() {
        let commands = handover_commands(&example());
        let joined = commands.join("\n");
        assert!(joined.contains("systemctl --user enable --now"));
        assert!(joined.contains("后悔药"));
        assert!(joined.contains(&guard::OWN_UNIT));
    }

    #[test]
    fn port_parsing_handles_host_and_port() {
        assert_eq!(port_of("127.0.0.1:1234"), "1234");
        assert_eq!(port_of("[::1]:8080"), "8080");
        assert_eq!(port_of("garbage"), "");
    }
}
