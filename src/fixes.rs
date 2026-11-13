//! `gnome-fixes`: one page-level extension bundling the small desktop patches
//! that live under `/home/root/GNOME/`.
//!
//! The page is a thin, honest shell: every probe runs the *real* external tool
//! and shows its stdout. Nothing is cached, no system state is invented, and
//! dangerous flags (`--now`, `--restart`, `--run --yes`) are never invoked.
//!
//! Manifest: `extensions/gnome-fixes/manifest.toml`.

use std::path::PathBuf;
use std::process::Command;

/// Where all five upstream tools live. Override for tests / other machines.
pub const ROOT_ENV: &str = "GNOUS_FIXES_ROOT";
pub const DEFAULT_ROOT: &str = "/home/root/GNOME";

pub fn root() -> PathBuf {
    PathBuf::from(std::env::var(ROOT_ENV).unwrap_or_else(|_| DEFAULT_ROOT.to_string()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Check,
    Apply,
    Revert,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Action::Check => "体检",
            Action::Apply => "应用",
            Action::Revert => "回退",
        }
    }

    pub fn tag(self) -> &'static str {
        match self {
            Action::Check => "CHECK",
            Action::Apply => "APPLY",
            Action::Revert => "REVERT",
        }
    }
}

/// One row in the fixes list. `command` is built from `root()` on purpose:
/// moving the toolkit should only require changing one environment variable.
#[derive(Clone, Debug)]
pub struct Fix {
    pub id: &'static str,
    pub number: &'static str,
    pub name: &'static str,
    pub subtitle: &'static str,
    pub why: &'static str,
    pub command: &'static str,
    pub cells: [(&'static str, &'static str); 4],
    pub commands: [(&'static str, &'static str); 3],
}

const fn cells(
    a: (&'static str, &'static str),
    b: (&'static str, &'static str),
    c: (&'static str, &'static str),
    d: (&'static str, &'static str),
) -> [(&'static str, &'static str); 4] {
    [a, b, c, d]
}

/// Static inventory. Adding a sixth patch = one more const + one manifest
/// entry (the manifest's `views` stays unchanged).
pub const FIXES: &[Fix] = &[
    Fix {
        id: "clipfix",
        number: "01",
        name: "剪贴板",
        subtitle: "终端里能复制、粘不出来 · Bug补丁/复制相关",
        why: "用户态拦不住「应用 → compositor」这一步，Wayland 剪贴板只能靠重启会话恢复。这里的补丁是绕行：让终端走 X11，使用没坏的 Xwayland 通道。",
        command: "systemctl --user set-environment GDK_BACKEND=x11",
        cells: cells(
            ("X11 → X11", "Xwayland 内部正常"),
            ("X11 → Wayland", "owner = mutter 隐形窗口"),
            ("Wayland → X11", "无 send，几秒后 cancelled"),
            ("Wayland → Wayland", "set_selection 后不取数据"),
        ),
        commands: [
            ("check", "./check.sh"),
            ("apply", "./on.sh"),
            ("revert", "./off.sh"),
        ],
    },
    Fix {
        id: "display",
        number: "02",
        name: "多屏幕",
        subtitle: "有外接屏只用外接屏 · 多屏幕/display-mode",
        why: "通过 mutter DisplayConfig 运行时应用布局；单改 monitors.xml 关不掉内置屏，mutter 会把缺失的显示器自动打开。开 = 自动单屏，回退 = 双屏扩展。",
        command: "display-mode auto",
        cells: cells(
            ("物理接口", "HDMI-1 / eDP-1"),
            ("当前布局", "由 status 决定"),
            ("自动对账", "systemd user timer"),
            ("手动调整", "先停 timer"),
        ),
        commands: [
            ("check", "python3 display-mode status"),
            ("apply", "python3 display-mode auto"),
            ("revert", "python3 display-mode both"),
        ],
    },
    Fix {
        id: "termfont",
        number: "03",
        name: "终端字体",
        subtitle: "GNOME 终端与 FinalShell 字号统一 · 终端字体",
        why: "GNOME 终端用 pt、FinalShell 用 px，且后者只在启动时读取；显示偏小的根源是两套单位。应用 = 统一为 12 pt / 16 px，回退 = 终端跟随系统等宽字体。",
        command: "termfont all 12",
        cells: cells(
            ("系统等宽字体", "gsettings monospace-font-name"),
            ("GNOME 终端", "pt · 改完立即生效"),
            ("FinalShell", "px · 下次启动生效"),
            ("换算", "1 pt = 4/3 px"),
        ),
        commands: [
            ("check", "./target/release/termfont status"),
            ("apply", "./target/release/termfont all 12"),
            ("revert", "./target/release/termfont term system"),
        ],
    },
    Fix {
        id: "rootdesk",
        number: "04",
        name: "Root 桌面对齐",
        subtitle: "root 会话缺 PipeWire / 全局搜索 · Root用户桌面功能对齐",
        why: "发行版 user 单元用 ConditionUser=!root 跳过整个媒体栈；本工具只在 ~/.config/systemd/user 写空赋值覆盖，不改 /usr/lib、不影响普通用户，删掉目录即可还原。",
        command: "root-gnome-align apply",
        cells: cells(
            ("PipeWire", "音频路由核心"),
            ("wireplumber", "设备枚举 / 音量"),
            ("mpris-proxy", "媒体键与蓝牙控制"),
            ("localsearch-3", "全局搜索索引"),
        ),
        commands: [
            ("check", "./root-gnome-align.sh status"),
            ("apply", "./root-gnome-align.sh apply"),
            ("revert", "./root-gnome-align.sh revert"),
        ],
    },
    Fix {
        id: "proxy",
        number: "05",
        name: "代理一致性",
        subtitle: "GUI 与终端代理变量同步 · 代理工具",
        why: "数据面是常驻的 j1900-proxy.service，控制面是 gsettings + environment.d；no_proxy 必须枚举 /24 的精确 IP，CIDR 对 Node/Bun 无效。应用/回退都会打印真实自检。",
        command: "./on.sh",
        cells: cells(
            ("中继", "127.0.0.1:31001"),
            ("上游", "192.168.1.7:31001"),
            ("GUI 代理", "gsettings mode"),
            ("终端 env", "environment.d"),
        ),
        commands: [
            ("check", "python3 jproxy.py check"),
            ("apply", "./on.sh"),
            ("revert", "./off.sh"),
        ],
    },
];

/// Result of one external command. `ok` is the process exit status; stdout +
/// stderr are kept verbatim so the log panel is never a paraphrase.
#[derive(Clone, Debug)]
pub struct Outcome {
    pub action: Action,
    #[allow(dead_code)]
    pub fix_id: &'static str,
    pub ok: bool,
    pub output: String,
}

fn dir_for(id: &'static str) -> &'static str {
    match id {
        "clipfix" => "Bug补丁/复制相关",
        "display" => "多屏幕",
        "termfont" => "终端字体",
        "rootdesk" => "Root用户桌面功能对齐",
        "proxy" => "代理工具",
        _ => "",
    }
}

/// Resolve a `Fix`'s command line into an OS command. The `check` command of
/// the proxy tool is a *dry run* — it never touches real state.
pub fn command_line(fix: &Fix, action: Action) -> Option<(PathBuf, Vec<String>)> {
    let raw = fix
        .commands
        .iter()
        .find(|(key, _)| *key == action.tag().to_ascii_lowercase().as_str())
        .map(|(_, cmd)| *cmd)?;
    let mut parts = raw.split_whitespace();
    let program = parts.next()?;
    let mut args: Vec<String> = parts.map(str::to_string).collect();
    let exe = if program == "./target/release/termfont" {
        root().join("终端字体/target/release/termfont")
    } else if program == "python3" {
        // Keep the script's own shebang/working directory intact.
        PathBuf::from("python3")
    } else if program.starts_with("./") {
        let dir = root().join(dir_for(fix.id));
        if program == "./root-gnome-align.sh" {
            dir.join("root-gnome-align.sh")
        } else {
            // Shell wrapper: run through bash so it works even when the
            // executable bit is missing in a fresh checkout.
            args.insert(0, dir.join(&program[2..]).to_string_lossy().into_owned());
            PathBuf::from("bash")
        }
    } else {
        PathBuf::from(program)
    };
    Some((exe, args))
}

/// Run one action. Called from a background executor, never on the UI thread.
pub fn run(fix_id: &'static str, action: Action) -> Outcome {
    let Some(fix) = FIXES.iter().find(|f| f.id == fix_id) else {
        return Outcome {
            action,
            fix_id,
            ok: false,
            output: format!("未知修复项: {fix_id}"),
        };
    };
    let Some((exe, args)) = command_line(fix, action) else {
        return Outcome {
            action,
            fix_id,
            ok: false,
            output: "命令未配置".to_string(),
        };
    };
    if !exe.exists() && exe.is_absolute() {
        return Outcome {
            action,
            fix_id,
            ok: false,
            output: format!(
                "工具未安装：{}\n\n设置 {ROOT_ENV} 指向工具目录。",
                exe.display()
            ),
        };
    }
    let result = Command::new(&exe)
        .args(&args)
        .current_dir(root().join(dir_for(fix.id)))
        .output();
    let (ok, output) = match result {
        Ok(out) => {
            let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
            let err = String::from_utf8_lossy(&out.stderr);
            if !err.is_empty() {
                text.push_str("\n--- stderr ---\n");
                text.push_str(&err);
            }
            if text.trim().is_empty() {
                text = format!("{} 已执行（无输出）", exe.display());
            }
            (out.status.success(), text)
        }
        Err(error) => (false, format!("启动 {} 失败：{error}", exe.display())),
    };
    Outcome {
        action,
        fix_id,
        ok,
        output,
    }
}

/// Per-row UI state, owned by `App`. `output` is the verbatim tool stdout;
/// `status` is derived from it and never invented.
#[derive(Clone, Debug)]
pub struct FixState {
    pub id: &'static str,
    pub status: Status,
    pub output: String,
    pub last: Option<Outcome>,
    pub expanded: bool,
    pub busy: bool,
}

impl FixState {
    pub fn empty(id: &'static str) -> Self {
        Self {
            id,
            status: Status::Unknown,
            output: String::new(),
            last: None,
            expanded: false,
            busy: false,
        }
    }

    pub fn absorb(&mut self, outcome: Outcome) {
        self.status = if outcome.ok {
            status(self.id, &outcome.output)
        } else {
            Status::Attention
        };
        self.output = outcome.output.clone();
        self.last = Some(outcome);
        self.busy = false;
    }
}

/// Parse the *real* probe output into a short row state. Deliberately
/// conservative: unknown output is `Unknown`, never a confident green.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Healthy,
    Attention,
    Applied,
    Unknown,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Healthy => "正常",
            Status::Attention => "待处理",
            Status::Applied => "已应用",
            Status::Unknown => "未体检",
        }
    }
}

pub fn status(fix_id: &str, output: &str) -> Status {
    match fix_id {
        "clipfix" => {
            if output.contains("补丁状态") && output.contains("环境变量不在") {
                Status::Attention
            } else if output.contains("BROKEN") || output.contains("现在是**坏的**") {
                Status::Attention
            } else if output.contains("回环正常") {
                Status::Healthy
            } else if output.contains("GDK_BACKEND=x11") {
                Status::Applied
            } else {
                Status::Unknown
            }
        }
        "display" => {
            if output.contains("[ON ] HDMI-1") && output.contains("[OFF] eDP-1") {
                Status::Applied
            } else if output.contains("[ON ]") {
                Status::Healthy
            } else {
                Status::Unknown
            }
        }
        "termfont" => {
            if output.contains("12 pt") && output.contains("16 px") {
                Status::Applied
            } else if output.contains("pt") {
                Status::Healthy
            } else {
                Status::Unknown
            }
        }
        "rootdesk" => {
            if output.contains("已完全对齐") {
                Status::Applied
            } else if output.contains("未完全对齐") {
                Status::Attention
            } else {
                Status::Unknown
            }
        }
        "proxy" => {
            // `jproxy.py check` is a read-only end-to-end probe: its
            // success/failure is the source of truth, never the process state.
            if output.contains("代理已启用") || output.contains("中继已就绪") {
                Status::Applied
            } else if output.contains("代理可用") || output.contains("OK") {
                Status::Healthy
            } else if output.contains("未运行") || output.contains("不可达") {
                Status::Attention
            } else {
                Status::Unknown
            }
        }
        _ => Status::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inventory_is_complete() {
        assert_eq!(FIXES.len(), 5);
        let mut ids = FIXES.iter().map(|f| f.id).collect::<Vec<_>>();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 5);
        for fix in FIXES {
            assert_eq!(fix.commands.len(), 3);
            assert_eq!(fix.cells.len(), 4);
        }
    }

    #[test]
    fn every_action_has_a_command() {
        for fix in FIXES {
            for action in [Action::Check, Action::Apply, Action::Revert] {
                assert!(command_line(fix, action).is_some(), "{} {action:?}", fix.id);
            }
        }
    }

    #[test]
    fn status_never_fakes_health() {
        assert_eq!(status("clipfix", ""), Status::Unknown);
        assert_eq!(
            status("clipfix", "Wayland 回环自测: BROKEN"),
            Status::Attention
        );
        assert_eq!(status("rootdesk", "状态：已完全对齐 ✅"), Status::Applied);
        assert_eq!(status("unknown", "anything"), Status::Unknown);
    }
}
