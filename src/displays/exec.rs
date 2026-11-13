//! 执行外部命令的唯一入口，并且只允许只读的 systemd 查询。
//!
//! 这个扩展**不需要**任何写入型外部命令：真正的显示操作走 D-Bus，单元文件是
//! GNOUS 自己写的普通文件，交棒命令只打印。所以这里的允许清单短到可以逐条读完。

use std::time::Duration;

use crate::fixes_backend::process;

const TIMEOUT: Duration = Duration::from_secs(6);

/// 允许清单：只读查询。
/// `systemctl --user show/is-active <unit>`、`systemctl --user cat <unit>`。
pub const ALLOWED: [&[&str]; 2] = [
    &["systemctl", "--user", "show"],
    &["systemctl", "--user", "is-active"],
];

pub fn check(argv: &[String]) -> Result<(), String> {
    if argv.len() < 3 {
        return Err(format!("命令过短，拒绝执行：{argv:?}"));
    }
    let prefix = ALLOWED
        .iter()
        .find(|allowed| argv.len() >= allowed.len() && argv[..allowed.len()] == allowed[..]);
    let Some(_prefix) = prefix else {
        return Err(format!("不在允许清单里，拒绝执行：{}", argv.join(" ")));
    };
    // 只读子命令之后不允许再出现会改状态的 systemctl 动作。
    const MUTATING: [&str; 9] = [
        "start",
        "stop",
        "restart",
        "reload",
        "enable",
        "disable",
        "mask",
        "kill",
        "set-property",
    ];
    if let Some(bad) = argv.iter().find(|arg| MUTATING.contains(&arg.as_str())) {
        return Err(format!("拒绝执行带副作用的 systemctl 动作：{bad}"));
    }
    // 允许清单本身占用的前缀（例如 `--user`）跳过，只检查后面的参数。
    for arg in &argv[3..] {
        if arg.starts_with('-') && !arg.starts_with("-p") {
            return Err(format!("拒绝未知参数：{arg}（只允许 -p）"));
        }
    }
    Ok(())
}

pub fn run(argv: &[&str]) -> Result<String, String> {
    let owned: Vec<String> = argv.iter().map(|arg| (*arg).to_string()).collect();
    check(&owned)?;
    let (program, rest) = owned.split_first().ok_or("空命令")?;
    let borrowed: Vec<&str> = rest.iter().map(String::as_str).collect();
    // `is-active` 对 inactive 单元返回 3，`process::run_bounded` 会把它当失败并
    // 带上 stdout —— 调用方按"文本里有没有 active"判断，不把退出码当结论。
    process::run_bounded(program, &borrowed, TIMEOUT)
}

// 失败也要拿到 stdout：`process::run_bounded` 出错时会带上输出，正常路径不会到这里。
#[cfg(test)]
mod tests {
    use super::*;

    fn argv(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn allows_read_only_probes() {
        assert!(
            check(&argv(&[
                "systemctl",
                "--user",
                "show",
                "display-mode.timer",
                "-p",
                "ActiveState"
            ]))
            .is_ok()
        );
        assert!(check(&argv(&["systemctl", "--user", "is-active", "x.service"])).is_ok());
    }

    #[test]
    fn refuses_mutating_systemctl() {
        for verb in [
            "stop",
            "start",
            "restart",
            "disable",
            "enable",
            "daemon-reload",
        ] {
            let err = check(&argv(&["systemctl", "--user", verb, "display-mode.timer"]))
                .expect_err("必须拒绝");
            assert!(err.contains("拒绝"), "{verb}: {err}");
        }
    }

    #[test]
    fn refuses_other_programs() {
        assert!(check(&argv(&["rm", "-rf", "/"])).is_err());
        assert!(check(&argv(&["xrandr", "--output", "HDMI-1"])).is_err());
        assert!(check(&argv(&["python3", "/home/root/GNOME/多屏幕/display-mode"])).is_err());
    }

    #[test]
    fn refuses_unknown_flags() {
        // 允许 -p，不允许 --now 这类危险标志。
        assert!(
            check(&argv(&[
                "systemctl",
                "--user",
                "show",
                "u.service",
                "--now"
            ]))
            .is_err()
        );
    }
}
