//! `termfont`（终端字体）页面级扩展的内建后端。
//!
//! 它替代 `/home/root/GNOME/终端字体/termfont` 那只小命令行：同样的四件事，
//! 但状态是结构化的，写之前先备份，改完重新读一次真实值再报结论。
//!
//! | 标签 | 操作 | 会不会改机器 |
//! | --- | --- | --- |
//! | 诊断 | `status` / `doctor` | 不会，只读 gsettings 与 FinalShell 配置 |
//! | 终端 | `term` | 会，改终端 profile 的 `font` |
//! | FinalShell | `fs` | 会，定点改 `config.json` 的 `font_size`（先备份） |
//! | 一起 | `all` | 会，两者都改，px 由 pt 换算 |
//! | 恢复 | `follow` | 会，终端改回"跟随系统等宽字体" |
//! | 危险 | `restart` | 会，重启 FinalShell——**断开所有 SSH 会话** |
//!
//! 三条安全约定：
//!
//! 1. 只写终端 profile 的 `font` / `use-system-font`，别的一个键都不碰（见 `exec`）；
//! 2. 改 FinalShell 配置前一定先备份成 `config.json.bak`，且只做定点文本替换，
//!    不重新序列化——文件里有加密字段；
//! 3. 重启 FinalShell 永远不会自动发生：`fs` / `all` 只提示"下次启动生效"，
//!    只有人再点一次 `RESTART` 并二次确认才动。

pub mod config;
pub mod exec;
pub mod finalshell;
pub mod gsettings;

use serde::Serialize;

use config::{Targets, fmt_num, px2pt};
use gsettings::TermFont;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Status,
    Doctor,
    Term,
    Fs,
    All,
    Follow,
    Restart,
}

impl Op {
    pub const ALL: [Op; 7] = [
        Op::Status,
        Op::Doctor,
        Op::Term,
        Op::Fs,
        Op::All,
        Op::Follow,
        Op::Restart,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Op::Status => "status",
            Op::Doctor => "doctor",
            Op::Term => "term",
            Op::Fs => "fs",
            Op::All => "all",
            Op::Follow => "follow",
            Op::Restart => "restart",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Op::Status => "STATUS",
            Op::Doctor => "DOCTOR",
            Op::Term => "TERM",
            Op::Fs => "FS",
            Op::All => "ALL",
            Op::Follow => "FOLLOW",
            Op::Restart => "RESTART",
        }
    }

    /// 会改机器状态的是这五个；`status` / `doctor` 只读。
    pub fn is_mutating(self) -> bool {
        !matches!(self, Op::Status | Op::Doctor)
    }

    /// `restart` 会断掉正在跑的 SSH 会话，UI 上单独标红。
    pub fn is_dangerous(self) -> bool {
        matches!(self, Op::Restart)
    }

    pub fn describe(self, targets: &Targets) -> String {
        match self {
            Op::Status => "读当前字号".to_string(),
            Op::Doctor => "八项自检".to_string(),
            Op::Term => format!("终端 → {} pt", fmt_num(targets.term_pt)),
            Op::Fs => format!("FinalShell → {} px", targets.fs_px),
            Op::All => format!(
                "终端 {} pt + FinalShell {} px",
                fmt_num(targets.term_pt),
                targets.convert()
            ),
            Op::Follow => "终端 → 跟随系统".to_string(),
            Op::Restart => "重启 FinalShell".to_string(),
        }
    }
}

/* ---------------------------------- facts --------------------------------- */

#[derive(Debug, Clone, Default, Serialize)]
pub struct CheckFacts {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TermFacts {
    pub raw: String,
    pub family: String,
    pub pt: Option<f64>,
    pub use_system: bool,
    pub effective_pt: Option<f64>,
    pub effective_family: String,
    pub profile: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct FsFacts {
    pub path: String,
    pub present: bool,
    pub px: Option<i64>,
    pub running: bool,
    pub pids: usize,
    pub backup: String,
    pub backup_present: bool,
    pub binary_present: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TargetFacts {
    pub path: String,
    /// `12 pt / 16 px · 换算 开`，给人看的一行。
    pub summary_line: String,
    pub term_pt: f64,
    pub fs_px: i64,
    pub convert_px: i64,
    pub auto_convert: bool,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct StatusFacts {
    /// `healthy` / `attention` / `degraded`。
    pub headline: String,
    pub headline_reason: String,
    pub system_raw: String,
    pub system_family: String,
    pub system_pt: Option<f64>,
    pub system_error: Option<String>,
    pub term: TermFacts,
    pub fs: FsFacts,
    pub targets: TargetFacts,
    /// 终端实际字号是否已经等于目标。
    pub term_matches: bool,
    /// FinalShell 配置里的 px 是否已经等于目标。
    pub fs_matches: bool,
    /// FinalShell 正在跑且配置已改：此时需要 RESTART 才看得到新字号。
    pub fs_pending_restart: bool,
    pub checks: Vec<CheckFacts>,
    pub dry_run: bool,
}

#[derive(Debug, Clone)]
pub struct Outcome {
    pub op: Op,
    pub ok: bool,
    pub output: String,
    pub facts: Option<StatusFacts>,
    /// 真的落盘/落 gsettings 时为真；dry-run 与幂等都为假。
    pub changed: bool,
}

impl Outcome {
    pub fn render(&self) -> String {
        self.output.trim_end().to_string()
    }
}

/* -------------------------------- collection ------------------------------- */

/// 一次采集的原始输入。判断逻辑（`build_facts`）因此可以是纯函数。
pub struct Inputs {
    pub targets: Targets,
    pub targets_warning: Option<String>,
    pub system: Result<(String, Option<f64>), String>,
    pub term: Result<TermFont, String>,
    pub profile: Result<String, String>,
    pub fs: Result<finalshell::Config, String>,
    pub fs_pids: usize,
    pub fs_binary_present: bool,
}

pub fn collect() -> Inputs {
    let (targets, targets_warning) = config::load();
    let system = gsettings::system_monospace().map(|(family, pt)| {
        let raw = match pt {
            Some(pt) => format!("{family} {}", fmt_num(pt)),
            None => family,
        };
        (raw, pt)
    });
    Inputs {
        targets,
        targets_warning,
        system,
        term: gsettings::term_font(),
        profile: gsettings::default_profile(),
        fs: finalshell::read(),
        fs_pids: finalshell::running_pids().len(),
        fs_binary_present: std::path::Path::new(&finalshell::bin()).exists(),
    }
}

/// 纯函数：把采集到的输入变成 UI 要的 facts。
pub fn build_facts(inputs: &Inputs) -> StatusFacts {
    let system_error = inputs.system.as_ref().err().cloned();
    let (system_raw, system_family, system_pt) = match &inputs.system {
        Ok((raw, pt)) => (raw.clone(), gsettings::split_font(raw).0, *pt),
        Err(_) => (String::new(), String::new(), None),
    };

    let term = match &inputs.term {
        Ok(font) => TermFacts {
            raw: font.raw.clone(),
            family: font.family.clone(),
            pt: font.pt,
            use_system: font.use_system,
            effective_pt: font.effective_pt(system_pt),
            effective_family: font.effective_family(&system_family),
            profile: inputs.profile.clone().unwrap_or_default(),
            error: None,
        },
        Err(error) => TermFacts {
            error: Some(error.clone()),
            profile: inputs.profile.clone().unwrap_or_default(),
            ..TermFacts::default()
        },
    };

    let (fs_px, fs_error, fs_present) = match &inputs.fs {
        Ok(config) => (Some(config.px), None, true),
        Err(error) => (None, Some(error.clone()), false),
    };

    let fs = FsFacts {
        path: finalshell::config_path().display().to_string(),
        present: fs_present,
        px: fs_px,
        running: inputs.fs_pids > 0,
        pids: inputs.fs_pids,
        backup: finalshell::backup_path().display().to_string(),
        backup_present: finalshell::backup_path().exists(),
        binary_present: inputs.fs_binary_present,
        error: fs_error,
    };

    let targets = TargetFacts {
        path: config::config_path().display().to_string(),
        summary_line: inputs.targets.summary(),
        term_pt: inputs.targets.term_pt,
        fs_px: inputs.targets.fs_px,
        convert_px: inputs.targets.convert(),
        auto_convert: inputs.targets.auto_convert,
        warning: inputs.targets_warning.clone(),
    };

    // pt 有半步，直接比较浮点会有 11.999999 这种假不一致。
    let term_matches = match term.effective_pt {
        Some(pt) => (pt - targets.term_pt).abs() < 0.01,
        None => false,
    };
    let fs_matches = fs.px == Some(targets.convert_px) || fs.px == Some(targets.fs_px);
    let fs_pending_restart = fs.running && !fs_matches;

    let checks = checks(inputs, &term, &fs);

    // 结论：先看"能不能看见真实值"，再看"跟目标一致吗"。
    let (headline, headline_reason) = if system_error.is_some() || term.error.is_some() {
        (
            "degraded".to_string(),
            format!(
                "读不到当前字号：{}",
                term.error
                    .clone()
                    .or(system_error.clone())
                    .unwrap_or_default()
            ),
        )
    } else if !fs.present {
        (
            "attention".to_string(),
            format!(
                "FinalShell 没装或没运行过，只有终端字号可管{}",
                if term_matches {
                    String::new()
                } else {
                    format!(
                        "；终端 {} pt ≠ 目标 {} pt",
                        term.effective_pt.map(fmt_num).unwrap_or_else(|| "?".into()),
                        fmt_num(targets.term_pt)
                    )
                }
            ),
        )
    } else if !term_matches && term.use_system {
        (
            "attention".to_string(),
            format!(
                "终端跟随系统（{} pt），与目标 {} pt 不一致",
                term.effective_pt.map(fmt_num).unwrap_or_else(|| "?".into()),
                fmt_num(targets.term_pt)
            ),
        )
    } else if !term_matches || !fs_matches {
        let mut parts = Vec::new();
        if !term_matches {
            parts.push(format!(
                "终端 {} pt ≠ 目标 {} pt",
                term.effective_pt.map(fmt_num).unwrap_or_else(|| "?".into()),
                fmt_num(targets.term_pt)
            ));
        }
        if !fs_matches {
            parts.push(format!(
                "FinalShell {} px ≠ 目标 {} px",
                fs.px.map(|px| px.to_string()).unwrap_or_else(|| "?".into()),
                targets.convert_px
            ));
        }
        ("attention".to_string(), parts.join(" · "))
    } else if fs_pending_restart {
        (
            "attention".to_string(),
            "配置已改，但 FinalShell 还在用旧字号，需要 RESTART 才生效".to_string(),
        )
    } else {
        (
            "healthy".to_string(),
            "两台终端的字号都等于目标".to_string(),
        )
    };

    StatusFacts {
        headline,
        headline_reason,
        system_raw,
        system_family,
        system_pt,
        system_error,
        term,
        fs,
        targets,
        term_matches,
        fs_matches,
        fs_pending_restart,
        checks,
        dry_run: config::dry_run(),
    }
}

fn checks(inputs: &Inputs, term: &TermFacts, fs: &FsFacts) -> Vec<CheckFacts> {
    let mut out = Vec::new();
    out.push(CheckFacts {
        name: "gsettings".to_string(),
        ok: inputs.system.is_ok(),
        detail: match &inputs.system {
            Ok((raw, _)) => format!("系统等宽 {raw}"),
            Err(error) => error.clone(),
        },
    });
    out.push(CheckFacts {
        name: "终端 profile".to_string(),
        ok: inputs.profile.is_ok(),
        detail: match &inputs.profile {
            Ok(uuid) => format!("默认 {uuid}"),
            Err(error) => error.clone(),
        },
    });
    out.push(CheckFacts {
        name: "终端字号".to_string(),
        ok: term.error.is_none(),
        detail: match &term.error {
            None => format!(
                "{}（{}）",
                term.raw,
                if term.use_system {
                    "跟随系统"
                } else {
                    "自定义"
                }
            ),
            Some(error) => error.clone(),
        },
    });
    out.push(CheckFacts {
        name: "实际生效字号".to_string(),
        ok: term.effective_pt.is_some(),
        detail: match term.effective_pt {
            Some(pt) => format!("{} {} pt", term.effective_family, fmt_num(pt)),
            None => "解析不出字号".to_string(),
        },
    });
    out.push(CheckFacts {
        name: "FinalShell 配置".to_string(),
        ok: fs.present,
        detail: match &fs.error {
            None => format!("{} · {} px", fs.path, fs.px.unwrap_or(0)),
            Some(error) => error.clone(),
        },
    });
    out.push(CheckFacts {
        name: "FinalShell 二进制".to_string(),
        ok: fs.binary_present,
        detail: if fs.binary_present {
            finalshell::bin()
        } else {
            format!("找不到 {}（只改配置，不会重启）", finalshell::bin())
        },
    });
    out.push(CheckFacts {
        name: "回滚备份".to_string(),
        ok: fs.backup_present || !fs.present,
        detail: if fs.backup_present {
            format!("备份在 {}", fs.backup)
        } else if fs.present {
            "还没有备份（第一次改 FinalShell 时自动生成）".to_string()
        } else {
            "没有可改的配置".to_string()
        },
    });
    out.push(CheckFacts {
        name: "进程".to_string(),
        ok: true,
        detail: if fs.running {
            format!(
                "FinalShell 运行中（{} 个进程），改完要重启用新字号",
                fs.pids
            )
        } else {
            "FinalShell 未运行，下次启动即为新字号".to_string()
        },
    });
    out
}

/* ---------------------------------- 输出 ---------------------------------- */

fn print_font_line(facts: &StatusFacts) -> String {
    let tag = if facts.term.use_system {
        "跟随系统".to_string()
    } else {
        "自定义".to_string()
    };
    format!(
        "GNOME 终端     : {} {} pt  [{}]{}",
        facts.term.effective_family,
        facts
            .term
            .effective_pt
            .map(fmt_num)
            .unwrap_or_else(|| "?".into()),
        tag,
        match &facts.term.raw {
            raw if raw.is_empty() => String::new(),
            raw if facts.term.use_system => format!("（profile 里写着 {raw}，但没生效）"),
            _ => String::new(),
        }
    )
}

fn print_fs_line(facts: &StatusFacts) -> String {
    if !facts.fs.present {
        return format!(
            "FinalShell     : 读不到配置{}",
            facts
                .fs
                .error
                .as_ref()
                .map(|e| format!("（{e}）"))
                .unwrap_or_default()
        );
    }
    let px = facts.fs.px.unwrap_or(0);
    format!(
        "FinalShell     : {} px ≈ {} pt  [{}]",
        px,
        fmt_num(px2pt(px)),
        if facts.fs.running {
            format!("运行中 {} 个进程", facts.fs.pids)
        } else {
            "未运行".to_string()
        }
    )
}

fn status_text(facts: &StatusFacts) -> String {
    let mut out = String::new();
    out.push_str("$ gnous builtin termfont / status\n");
    out.push_str(&format!(
        "结论: {} — {}\n",
        facts.headline.to_uppercase(),
        facts.headline_reason
    ));
    out.push_str(&format!("系统等宽字体   : {}\n", facts.system_raw));
    out.push_str(&print_font_line(facts));
    out.push('\n');
    out.push_str(&print_fs_line(facts));
    out.push('\n');
    out.push_str(&format!(
        "目标           : {} · 实际写入 {} px{}\n",
        facts.targets.summary_line,
        facts.targets.convert_px,
        if facts.targets.auto_convert {
            "（由 pt 换算）"
        } else {
            "（FinalShell 单独设的）"
        }
    ));
    out.push_str(&format!(
        "一致性         : 终端 {} · FinalShell {}\n",
        if facts.term_matches {
            "一致"
        } else {
            "不一致"
        },
        if facts.fs_matches {
            "一致"
        } else {
            "不一致"
        }
    ));
    out.push_str(&format!(
        "备份           : {}{}\n",
        facts.fs.backup,
        if facts.fs.backup_present {
            "（存在）"
        } else {
            "（还没有，第一次改 FinalShell 时生成）"
        }
    ));
    out.push_str(&format!(
        "配置           : {} · {} · {}\n",
        facts.targets.path,
        fmt_num(facts.targets.term_pt),
        facts.targets.fs_px
    ));
    if facts.fs.running && facts.fs_pending_restart {
        out.push_str("注意           : FinalShell 运行中，改字号后需要 RESTART 才生效\n");
    }
    if facts.dry_run {
        out.push_str("[dry-run] 本次不写任何文件\n");
    }
    out
}

fn doctor_text(facts: &StatusFacts) -> String {
    let mut out = String::new();
    out.push_str("$ gnous builtin termfont / doctor\n");
    let passed = facts.checks.iter().filter(|check| check.ok).count();
    out.push_str(&format!("自检: {passed} / {} 通过\n", facts.checks.len()));
    for check in &facts.checks {
        out.push_str(&format!(
            "{} {:<18} {}\n",
            if check.ok { "✓" } else { "✗" },
            check.name,
            check.detail
        ));
    }
    out.push_str(&format!(
        "\n只写终端 profile 的 font / use-system-font；FinalShell 只做定点文本替换，写前备份到 {}。\n",
        facts.fs.backup
    ));
    out
}

/* ----------------------------------- run ---------------------------------- */

pub fn run(op: Op) -> Outcome {
    let before = collect();
    let before_facts = build_facts(&before);
    let dry_run = config::dry_run();
    let targets = before.targets;

    let mut changed = false;
    let mut output = String::new();

    match op {
        Op::Status => {
            return Outcome {
                op,
                ok: true,
                output: status_text(&before_facts),
                facts: Some(before_facts),
                changed: false,
            };
        }
        Op::Doctor => {
            return Outcome {
                op,
                ok: true,
                output: doctor_text(&before_facts),
                facts: Some(before_facts),
                changed: false,
            };
        }
        Op::Term => {
            output.push_str(&format!(
                "$ gnous builtin termfont / term {}pt\n",
                fmt_num(targets.term_pt)
            ));
            match gsettings::set_pt(targets.term_pt) {
                Ok(value) => {
                    changed = true;
                    output.push_str(&format!("已写入 profile 的 font = {value}\n"));
                }
                Err(error) => output.push_str(&format!("✗ 写入失败: {error}\n")),
            }
        }
        Op::Fs => {
            output.push_str(&format!(
                "$ gnous builtin termfont / fs {}px\n",
                targets.fs_px
            ));
            match write_fs(targets.fs_px, dry_run) {
                Ok((old, wrote)) => {
                    changed = wrote;
                    output.push_str(&format!("FinalShell: {old} px -> {} px", targets.fs_px));
                    output.push_str(if wrote { "" } else { "（无变化）" });
                    output.push('\n');
                    output.push_str(&format!("备份: {}\n", finalshell::backup_path().display()));
                }
                Err(error) => output.push_str(&format!("✗ {error}\n")),
            }
        }
        Op::All => {
            let px = targets.convert();
            output.push_str(&format!(
                "$ gnous builtin termfont / all {}pt ({}px)\n",
                fmt_num(targets.term_pt),
                px
            ));
            match gsettings::set_pt(targets.term_pt) {
                Ok(value) => {
                    changed = true;
                    output.push_str(&format!("终端 font = {value}\n"));
                }
                Err(error) => output.push_str(&format!("✗ 终端写入失败: {error}\n")),
            }
            match write_fs(px, dry_run) {
                Ok((old, wrote)) => {
                    changed |= wrote;
                    output.push_str(&format!("FinalShell: {old} px -> {px} px\n"));
                }
                Err(error) => output.push_str(&format!("✗ FinalShell: {error}\n")),
            }
        }
        Op::Follow => {
            output.push_str("$ gnous builtin termfont / follow\n");
            match gsettings::follow_system() {
                Ok(()) => {
                    changed = true;
                    output.push_str("终端已恢复为跟随系统等宽字体\n");
                }
                Err(error) => output.push_str(&format!("✗ {error}\n")),
            }
        }
        Op::Restart => {
            output.push_str("$ gnous builtin termfont / restart\n");
            if dry_run {
                output.push_str("[dry-run] 跳过重启（会断开 SSH 会话）\n");
            } else {
                match finalshell::restart() {
                    Ok(killed) => {
                        changed = true;
                        output.push_str(&format!(
                            "已发送 SIGTERM 给 {killed} 个进程并重新拉起 {}\n",
                            finalshell::bin()
                        ));
                        output.push_str("注意: 原有 SSH 会话已断开，重连即可。\n");
                    }
                    Err(error) => output.push_str(&format!("✗ {error}\n")),
                }
            }
        }
    }

    // 不管改没改，都重新读一次真实值：这才是面板上的结论依据。
    let after = collect();
    let facts = build_facts(&after);
    if op != Op::Restart && !output.is_empty() {
        output.push('\n');
        output.push_str(&status_text(&facts));
    }
    let ok = !output.contains("✗");
    Outcome {
        op,
        ok,
        output,
        facts: Some(facts),
        changed,
    }
}

fn write_fs(px: i64, dry_run: bool) -> Result<(i64, bool), String> {
    if dry_run {
        let current = finalshell::read()?;
        return Ok((current.px, false));
    }
    let (old, _backup) = finalshell::write_px(px)?;
    Ok((old, old != px))
}

/* ------------------------------- ui 状态 --------------------------------- */

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Status,
    Doctor,
    Term,
    Fs,
    All,
    Follow,
    Restart,
}

impl Tab {
    pub const ALL: [Tab; 7] = [
        Tab::Status,
        Tab::Doctor,
        Tab::Term,
        Tab::Fs,
        Tab::All,
        Tab::Follow,
        Tab::Restart,
    ];

    pub fn label(self) -> &'static str {
        self.op().label()
    }

    pub fn op(self) -> Op {
        match self {
            Tab::Status => Op::Status,
            Tab::Doctor => Op::Doctor,
            Tab::Term => Op::Term,
            Tab::Fs => Op::Fs,
            Tab::All => Op::All,
            Tab::Follow => Op::Follow,
            Tab::Restart => Op::Restart,
        }
    }

    fn of(op: Op) -> Tab {
        match op {
            Op::Status => Tab::Status,
            Op::Doctor => Tab::Doctor,
            Op::Term => Tab::Term,
            Op::Fs => Tab::Fs,
            Op::All => Tab::All,
            Op::Follow => Tab::Follow,
            Op::Restart => Tab::Restart,
        }
    }
}

#[derive(Clone, Debug)]
pub struct State {
    pub tab: Tab,
    pub busy: bool,
    /// 需要第二次点击确认的 op。
    pub armed: Option<Op>,
    pub outcomes: Vec<(Op, Outcome)>,
    pub facts: Option<StatusFacts>,
    /// 左列高亮的面。
    pub face: usize,
    /// 步进目标值时出的错（例如写配置失败）。
    pub edit_error: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

impl State {
    pub fn new() -> Self {
        State {
            tab: Tab::Status,
            busy: false,
            armed: None,
            outcomes: Vec::new(),
            facts: None,
            face: 0,
            edit_error: None,
        }
    }

    pub fn absorb(&mut self, outcome: Outcome, switch_tab: bool) {
        let op = outcome.op;
        if let Some(facts) = outcome.facts.clone() {
            self.facts = Some(facts);
        }
        self.outcomes.retain(|(existing, _)| *existing != op);
        self.outcomes.push((op, outcome));
        if switch_tab {
            self.tab = Tab::of(op);
        }
    }

    pub fn outcome(&self, tab: Tab) -> Option<&Outcome> {
        self.outcomes
            .iter()
            .rev()
            .find(|(op, _)| *op == tab.op())
            .map(|(_, outcome)| outcome)
    }

    pub fn has_status(&self) -> bool {
        self.outcome(Tab::Status).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs() -> Inputs {
        Inputs {
            targets: Targets::default(),
            targets_warning: None,
            system: Ok(("Monospace 11".to_string(), Some(11.0))),
            term: Ok(TermFont {
                family: "Monospace".to_string(),
                pt: Some(12.0),
                use_system: false,
                raw: "'Monospace 12'".to_string(),
            }),
            profile: Ok("b1dcc9dd-5262-4d8d-a863-c897e6d979b9".to_string()),
            fs: Ok(finalshell::Config {
                text: r#"{"font_size":16}"#.to_string(),
                px: 16,
            }),
            fs_pids: 0,
            fs_binary_present: true,
        }
    }

    #[test]
    fn everything_matching_is_healthy() {
        let facts = build_facts(&inputs());
        assert_eq!(facts.headline, "healthy", "{:?}", facts.headline_reason);
        assert!(facts.term_matches);
        assert!(facts.fs_matches);
        assert_eq!(facts.checks.iter().filter(|c| c.ok).count(), 8);
        assert!(status_text(&facts).contains("GNOME 终端"));
    }

    #[test]
    fn a_different_terminal_size_is_attention() {
        let mut inputs = inputs();
        inputs.term = Ok(TermFont {
            family: "Monospace".into(),
            pt: Some(9.0),
            use_system: false,
            raw: "'Monospace 9'".into(),
        });
        let facts = build_facts(&inputs);
        assert_eq!(facts.headline, "attention");
        assert!(!facts.term_matches);
        assert!(facts.headline_reason.contains("终端 9 pt ≠ 目标 12 pt"));
    }

    #[test]
    fn following_the_system_reports_the_system_size() {
        let mut inputs = inputs();
        inputs.term = Ok(TermFont {
            family: "Monospace".into(),
            pt: Some(12.0),
            use_system: true,
            raw: "'Monospace 12'".into(),
        });
        let facts = build_facts(&inputs);
        assert_eq!(
            facts.term.effective_pt,
            Some(11.0),
            "跟随系统时以系统值为准"
        );
        assert_eq!(facts.headline, "attention");
        assert!(facts.headline_reason.contains("跟随系统"));
    }

    #[test]
    fn missing_finalshell_is_attention_not_a_failure() {
        let mut inputs = inputs();
        inputs.fs = Err("读不到 /root/.finalshell/config.json".to_string());
        let facts = build_facts(&inputs);
        assert!(!facts.fs.present);
        assert_eq!(facts.headline, "attention");
        assert!(facts.headline_reason.contains("没装"));
        // 终端那半仍然要能显示。
        assert!(facts.term_matches);
    }

    #[test]
    fn a_running_finalshell_with_a_new_size_needs_a_restart() {
        let mut inputs = inputs();
        inputs.fs = Ok(finalshell::Config {
            text: r#"{"font_size":12}"#.to_string(),
            px: 12,
        });
        inputs.fs_pids = 2;
        let facts = build_facts(&inputs);
        assert!(facts.fs_pending_restart);
        assert_eq!(facts.headline, "attention");
        assert!(facts.headline_reason.contains("FinalShell 12 px"));
    }

    #[test]
    fn unreadable_gsettings_degrades_instead_of_guessing() {
        let mut inputs = inputs();
        inputs.system = Err("gsettings 执行失败".to_string());
        inputs.term = Err("gsettings 执行失败".to_string());
        let facts = build_facts(&inputs);
        assert_eq!(facts.headline, "degraded");
        assert!(facts.headline_reason.contains("gsettings"));
        assert_eq!(facts.term.effective_pt, None);
        assert!(doctor_text(&facts).contains("✗"));
    }

    #[test]
    fn doctor_lists_eight_checks_with_details() {
        let facts = build_facts(&inputs());
        let text = doctor_text(&facts);
        assert_eq!(facts.checks.len(), 8);
        assert!(text.contains("8 / 8 通过"));
        for check in &facts.checks {
            assert!(text.contains(&check.name), "{}", check.name);
        }
    }

    /// 真机集成测试：只要 gsettings 能读，facts 就必须是"看得见"的状态。
    /// 只读，不改机器。
    #[test]
    fn reads_the_real_terminal_font_when_gsettings_works() {
        let Ok(raw) = gsettings::system_monospace() else {
            return; // 无桌面会话时跳过
        };
        let facts = build_facts(&collect());
        assert!(facts.system_error.is_none());
        assert_eq!(facts.system_family, raw.0, "系统等宽字体族要对得上");
        assert!(facts.system_pt.is_some(), "系统等宽字体应有字号");
        assert!(
            facts.term.error.is_none(),
            "终端 profile 应该读得到：{:?}",
            facts.term.error
        );
        assert!(
            facts.term.effective_pt.is_some(),
            "实际生效字号必须解析出来"
        );
        // 结论只能是这三档之一，且必须带原因。
        assert!(
            ["healthy", "attention", "degraded"].contains(&facts.headline.as_str()),
            "{}",
            facts.headline
        );
        assert!(!facts.headline_reason.is_empty());
    }

    /// 真机集成测试：目标值与"现在"的比较是可重放的纯计算。
    #[test]
    fn comparing_against_the_real_state_is_consistent() {
        let inputs = collect();
        let facts = build_facts(&inputs);
        assert_eq!(
            facts.term_matches,
            facts
                .term
                .effective_pt
                .is_some_and(|pt| (pt - facts.targets.term_pt).abs() < 0.01),
            "term_matches 只能由真实字号算出来"
        );
        assert_eq!(
            facts.targets.convert_px,
            pt2px_for_test(facts.targets.term_pt)
        );
    }

    fn pt2px_for_test(pt: f64) -> i64 {
        (pt * 4.0 / 3.0).round() as i64
    }

    #[test]
    fn mutating_ops_are_the_five_that_write() {
        assert!(!Op::Status.is_mutating());
        assert!(!Op::Doctor.is_mutating());
        for op in [Op::Term, Op::Fs, Op::All, Op::Follow, Op::Restart] {
            assert!(op.is_mutating(), "{}", op.id());
        }
        assert!(Op::Restart.is_dangerous());
        assert!(!Op::Term.is_dangerous());
    }

    #[test]
    fn every_tab_maps_back_to_its_op() {
        for tab in Tab::ALL {
            assert_eq!(Tab::of(tab.op()), tab);
            assert_eq!(Op::ALL.iter().filter(|op| **op == tab.op()).count(), 1);
        }
    }

    #[test]
    fn describe_uses_the_targets() {
        let targets = Targets {
            term_pt: 13.0,
            fs_px: 20,
            auto_convert: true,
        };
        assert_eq!(Op::Term.describe(&targets), "终端 → 13 pt");
        assert_eq!(Op::All.describe(&targets), "终端 13 pt + FinalShell 17 px");
        assert_eq!(Op::Fs.describe(&targets), "FinalShell → 20 px");
    }
}
