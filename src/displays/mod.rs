//! `displays`（多屏幕）页面级扩展的内建后端。
//!
//! 它替代 `/home/root/GNOME/多屏幕/display-mode` 那套 bash+python 脚本：
//!
//! | 面 | 操作 | 会不会改变显示 |
//! | --- | --- | --- |
//! | 诊断 | `status` / `doctor` | 不会，只读 sysfs + D-Bus 查询 |
//! | 布局 | `preview` | 不会，走 mutter 的 `METHOD_VERIFY` 只校验 |
//! | 布局 | `apply` | 会，二次确认后写 mutter 的持久配置 |
//! | 接管 | `handover` | 不会，只打印人手执行的命令 |
//! | 接管 | `revert` | 不会，只删 GNOUS 自己的两只单元文件 |
//!
//! 和代理扩展的原则一致：老单元（`display-mode.timer`）只读，GNOUS 只写自己的
//! 文件，交棒命令只打印。区别是多屏幕不会把机器弄到连不上，所以 `apply` 允许
//! 由 UI 触发——但依然二次确认，且总是先跑一遍 verify 再真改。

pub mod config;
pub mod connectors;
pub mod dbus;
pub mod exec;
pub mod mutter;
pub mod units;

use serde::Serialize;

use config::{Config, Resolution};
use connectors::Connector;
use mutter::{Monitor, State as MutterState};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Op {
    Status,
    Doctor,
    Preview,
    Apply,
    Handover,
    Revert,
}

impl Op {
    pub const ALL: [Op; 6] = [
        Op::Status,
        Op::Doctor,
        Op::Preview,
        Op::Apply,
        Op::Handover,
        Op::Revert,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Op::Status => "status",
            Op::Doctor => "doctor",
            Op::Preview => "preview",
            Op::Apply => "apply",
            Op::Handover => "handover",
            Op::Revert => "revert",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Op::Status => "STATUS",
            Op::Doctor => "DOCTOR",
            Op::Preview => "PREVIEW",
            Op::Apply => "APPLY",
            Op::Handover => "HANDOVER",
            Op::Revert => "REVERT",
        }
    }

    /// 会改变机器状态的只有 `apply`（改显示）和 `revert`（删自己的单元文件）。
    pub fn is_mutating(self) -> bool {
        matches!(self, Op::Apply | Op::Revert)
    }

    pub fn is_read_only(self) -> bool {
        !self.is_mutating()
    }
}

/* ---------------------------------- facts --------------------------------- */

#[derive(Debug, Clone, Default, Serialize)]
pub struct ConnectorFacts {
    pub name: String,
    pub sysfs: String,
    pub in_mutter: bool,
    pub active: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct MonitorFacts {
    pub connector: String,
    pub product: String,
    pub current: Option<String>,
    pub preferred: Option<String>,
    pub mode_max: Option<String>,
    pub mode_count: usize,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct UnitFacts {
    pub present: bool,
    pub active: bool,
    pub enabled: bool,
    pub exec: String,
    pub line: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ConfigFacts {
    pub path: String,
    pub mode: String,
    pub resolution: String,
    pub external: String,
    pub internal: String,
    pub reconcile_secs: u64,
    pub takeover: bool,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CheckFacts {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct StatusFacts {
    /// `healthy` / `attention` / `degraded`。
    pub headline: String,
    pub headline_reason: String,
    pub bus_ok: bool,
    pub bus_error: Option<String>,
    pub serial: u32,
    pub connectors: Vec<ConnectorFacts>,
    pub monitors: Vec<MonitorFacts>,
    pub active: Vec<String>,
    pub desired: Vec<String>,
    pub drift: bool,
    pub legacy: UnitFacts,
    pub own: UnitFacts,
    pub config: ConfigFacts,
    pub checks: Vec<CheckFacts>,
    pub dry_run: bool,
}

#[derive(Debug, Clone)]
pub struct Outcome {
    pub op: Op,
    pub ok: bool,
    pub output: String,
    pub facts: Option<StatusFacts>,
    /// `apply` 实际改动了显示时为真；`preview` / 幂等时不改。
    pub changed: bool,
}

impl Outcome {
    pub fn render(&self) -> String {
        self.output.trim_end().to_string()
    }
}

/* -------------------------------- collection ------------------------------- */

/// 一次采集的全部原始输入。拆出来是为了让"判断"是纯函数、可单测。
pub struct Inputs {
    pub config: Config,
    pub config_warning: Option<String>,
    pub connectors: Vec<Connector>,
    pub state: Result<MutterState, String>,
    pub legacy_timer: units::UnitInfo,
    pub legacy_service: units::UnitInfo,
    pub own_timer: units::UnitInfo,
}

pub fn collect() -> Inputs {
    let (config, config_warning) = config::load();
    let connectors = connectors::scan(std::path::Path::new("/sys/class/drm"));
    let state = dbus::Conn::session()
        .map_err(|e| e.to_string())
        .and_then(|mut conn| mutter::get_state(&mut conn).map_err(|e| e.to_string()));
    Inputs {
        config,
        config_warning,
        connectors,
        state,
        legacy_timer: units::info(units::LEGACY_TIMER),
        legacy_service: units::info(units::LEGACY_SERVICE),
        own_timer: units::info(units::OWN_TIMER),
    }
}

fn unit_facts(info: &units::UnitInfo) -> UnitFacts {
    UnitFacts {
        present: info.present(),
        active: info.active(),
        enabled: info.enabled(),
        exec: info.exec(),
        line: info.one_line(),
    }
}

/// 外接屏"可用"的定义：sysfs 说插着，且 mutter 给出了可用模式。
fn external_usable(inputs: &Inputs, state: Option<&MutterState>) -> bool {
    let connected = connectors::is_connected(&inputs.connectors, &inputs.config.external);
    if !connected {
        return false;
    }
    match state {
        // mutter 不可用时按 sysfs 推断"意图"：面板上显示"插着外接屏，本来该用它"，
        // 而 apply 在这种状态下本来就做不了，所以这个推断不会造成误操作。
        None => true,
        Some(state) => state
            .monitor(&inputs.config.external)
            .and_then(Monitor::best_mode)
            .is_some(),
    }
}

fn pick_mode(monitor: &Monitor, resolution: Resolution) -> Option<mutter::Mode> {
    match resolution {
        Resolution::Preferred => monitor.best_mode().cloned(),
        Resolution::Max => monitor
            .modes
            .iter()
            .max_by_key(|m| (m.area(), (m.rate * 1000.0) as i64))
            .cloned(),
        Resolution::Current => monitor
            .modes
            .iter()
            .find(|m| m.current)
            .or_else(|| monitor.modes.iter().find(|m| m.preferred))
            .cloned(),
    }
}

/// 纯函数：把采集到的输入变成 UI 要的 facts。
pub fn build_facts(inputs: &Inputs) -> StatusFacts {
    let state = inputs.state.as_ref().ok();
    let bus_error = inputs.state.as_ref().err().cloned();
    let active = state.map(MutterState::active).unwrap_or_default();
    let desired = inputs.config.targets(external_usable(inputs, state));
    let mut desired_sorted = desired.clone();
    desired_sorted.sort();
    let mut active_sorted = active.clone();
    active_sorted.sort();
    let drift = !desired_sorted.is_empty() && active_sorted != desired_sorted;

    let connectors = inputs
        .connectors
        .iter()
        .map(|c| ConnectorFacts {
            name: c.name.clone(),
            sysfs: c.status.clone(),
            in_mutter: state.and_then(|s| s.monitor(&c.name)).is_some(),
            active: active.contains(&c.name),
        })
        .collect();

    let monitors = state
        .map(|s| {
            s.monitors
                .iter()
                .map(|m| MonitorFacts {
                    connector: m.connector.clone(),
                    product: format!("{} {}", m.vendor, m.product).trim().to_string(),
                    current: m
                        .modes
                        .iter()
                        .find(|mode| mode.current)
                        .map(mutter::Mode::label),
                    preferred: m
                        .modes
                        .iter()
                        .find(|mode| mode.preferred)
                        .map(mutter::Mode::label),
                    mode_max: m
                        .modes
                        .iter()
                        .max_by_key(|mode| mode.area())
                        .map(mutter::Mode::label),
                    mode_count: m.modes.len(),
                })
                .collect()
        })
        .unwrap_or_default();

    let legacy = unit_facts(&inputs.legacy_timer);
    let own = unit_facts(&inputs.own_timer);

    // 结论：先看"能不能看见"，再看"跟配置一致吗"，最后看"谁在管"。
    let (headline, headline_reason) = if !connections_ok(inputs) {
        (
            "degraded".to_string(),
            "看不到图形会话，无法确认显示状态".to_string(),
        )
    } else if drift {
        (
            "attention".to_string(),
            format!(
                "当前 {} 与期望 {} 不一致（{} 仍在接管）",
                if active_sorted.is_empty() {
                    "无".to_string()
                } else {
                    active_sorted.join("+")
                },
                desired_sorted.join("+"),
                if legacy.active && !inputs.config.takeover {
                    units::LEGACY_TIMER
                } else if own.active {
                    units::OWN_TIMER
                } else {
                    "没有 timer"
                }
            ),
        )
    } else if legacy.active && !inputs.config.takeover {
        (
            "attention".to_string(),
            format!(
                "布局符合配置，但仍是 {}（外部脚本）在对账",
                units::LEGACY_SERVICE
            ),
        )
    } else {
        ("healthy".to_string(), "布局符合配置".to_string())
    };

    let mut checks = Vec::new();
    checks.push(CheckFacts {
        name: "会话总线".into(),
        ok: inputs.state.is_ok(),
        detail: match &inputs.state {
            Ok(_) => "连上会话总线并能读 mutter 状态".into(),
            Err(error) => error.clone(),
        },
    });
    checks.push(CheckFacts {
        name: "物理接口".into(),
        ok: !inputs.connectors.is_empty(),
        detail: connectors::summary(&inputs.connectors),
    });
    checks.push(CheckFacts {
        name: "期望布局".into(),
        ok: !desired.is_empty() && state.is_some(),
        detail: format!(
            "{} → {}",
            inputs.config.mode.id(),
            if desired.is_empty() {
                "无".to_string()
            } else {
                desired.join("+")
            }
        ),
    });
    checks.push(CheckFacts {
        name: "当前布局".into(),
        ok: !active.is_empty(),
        detail: if active.is_empty() {
            "mutter 没有报告任何点亮的接口".to_string()
        } else {
            active.join("+")
        },
    });
    checks.push(CheckFacts {
        name: "一致性".into(),
        ok: !drift,
        detail: if drift {
            format!(
                "期望 {}，实际 {}",
                desired_sorted.join("+"),
                active_sorted.join("+")
            )
        } else {
            "实际布局等于期望布局".to_string()
        },
    });
    checks.push(CheckFacts {
        name: "老脚本".into(),
        ok: true,
        detail: format!("{}（GNOUS 只读它）", inputs.legacy_timer.one_line()),
    });
    checks.push(CheckFacts {
        name: "GNOUS 单元".into(),
        ok: true,
        detail: if inputs.own_timer.present() {
            inputs.own_timer.one_line()
        } else {
            "未安装（APPLY 只写文件，不启动）".to_string()
        },
    });
    checks.push(CheckFacts {
        name: "配置".into(),
        ok: inputs.config.validate().is_ok() && inputs.config_warning.is_none(),
        detail: inputs
            .config_warning
            .clone()
            .unwrap_or_else(|| inputs.config.summary()),
    });

    StatusFacts {
        headline,
        headline_reason,
        bus_ok: inputs.state.is_ok(),
        bus_error,
        serial: state.map(|s| s.serial).unwrap_or(0),
        connectors,
        monitors,
        active,
        desired,
        drift,
        legacy,
        own,
        config: ConfigFacts {
            path: config::config_path().display().to_string(),
            mode: inputs.config.mode.id().to_string(),
            resolution: inputs.config.resolution.id().to_string(),
            external: inputs.config.external.clone(),
            internal: inputs.config.internal.clone(),
            reconcile_secs: inputs.config.reconcile_secs,
            takeover: inputs.config.takeover,
            warning: inputs.config_warning.clone(),
        },
        checks,
        dry_run: config::dry_run(),
    }
}

fn connections_ok(inputs: &Inputs) -> bool {
    inputs.state.is_ok() && !inputs.connectors.is_empty()
}

/* ---------------------------------- output -------------------------------- */

fn status_text(facts: &StatusFacts, inputs: &Inputs) -> String {
    let mut lines = Vec::new();
    lines.push(format!("$ gnous builtin displays / status"));
    lines.push(format!(
        "结论: {} — {}",
        facts.headline.to_uppercase(),
        facts.headline_reason
    ));
    lines.push(format!(
        "物理接口: {}",
        connectors::summary(&inputs.connectors)
    ));
    if let Some(error) = &facts.bus_error {
        lines.push(format!("mutter: 不可用（{error}）"));
    } else {
        match &inputs.state {
            Ok(state) => {
                lines.push(format!(
                    "mutter: 序号 {} · 布局模式 {}",
                    state.serial,
                    match state.layout_mode {
                        Some(1) => "logical".to_string(),
                        Some(other) => format!("#{other}"),
                        None => "未知".to_string(),
                    }
                ));
                for line in state.describe().lines() {
                    lines.push(line.to_string());
                }
            }
            Err(_) => {}
        }
    }
    if facts.bus_error.is_some() {
        lines.push("（mutter 不可用，期望布局按 sysfs 推断，此状态下无法 apply）".to_string());
    }
    lines.push(format!(
        "期望: {} → {}",
        facts.config.mode,
        if facts.desired.is_empty() {
            "无".to_string()
        } else {
            facts.desired.join("+")
        }
    ));
    lines.push(format!(
        "实际: {}{}",
        if facts.active.is_empty() {
            "无".to_string()
        } else {
            facts.active.join("+")
        },
        if facts.drift {
            "（与期望不一致）"
        } else {
            "（一致）"
        }
    ));
    lines.push(format!("老脚本: {}", facts.legacy.line));
    lines.push(format!("GNOUS: {}", facts.own.line));
    lines.push(format!(
        "配置: {} · {}",
        facts.config.path,
        inputs.config.summary()
    ));
    if let Some(warning) = &facts.config.warning {
        lines.push(format!("配置警告: {warning}"));
    }
    if facts.dry_run {
        lines.push("dry-run: GNOUS_DISPLAY_DRY_RUN=1，不会真的改显示".to_string());
    }
    lines.join("\n")
}

fn doctor_text(facts: &StatusFacts) -> String {
    let mut lines = vec!["$ gnous builtin displays / doctor".to_string()];
    for check in &facts.checks {
        lines.push(format!(
            "  {} {:<6} {}",
            if check.ok { "✓" } else { "✗" },
            check.name,
            check.detail
        ));
    }
    let failed = facts.checks.iter().filter(|c| !c.ok).count();
    lines.push(format!(
        "{} 项检查，{} 项通过",
        facts.checks.len(),
        facts.checks.len() - failed
    ));
    lines.join("\n")
}

/* ----------------------------------- ops ---------------------------------- */

/// 读一次当前状态并规划目标布局。`preview` 与 `apply` 共用。
fn plan_current(inputs: &Inputs) -> Result<(MutterState, Vec<String>, dbus::V), String> {
    let state = inputs
        .state
        .as_ref()
        .map_err(|e| format!("读不到 mutter 状态：{e}"))?;
    if !connections_ok(inputs) {
        return Err("看不到图形会话或物理接口，无法规划布局".into());
    }
    let desired = inputs.config.targets(external_usable(inputs, Some(state)));
    let refs: Vec<&str> = desired.iter().map(String::as_str).collect();
    let resolution = inputs.config.resolution;
    let (logical, chosen) =
        mutter::plan_with(state, &refs, |monitor| pick_mode(monitor, resolution))?;
    Ok((state.clone(), chosen, logical))
}

fn preview(inputs: &Inputs, facts: &StatusFacts) -> Outcome {
    let mut lines = vec!["$ gnous builtin displays / preview".to_string()];
    match plan_current(inputs) {
        Err(error) => Outcome {
            op: Op::Preview,
            ok: false,
            output: format!("{}\n无法说明怎么改：{error}", lines.join("\n")),
            facts: Some(facts.clone()),
            changed: false,
        },
        Ok((state, chosen, logical)) => {
            lines.push(format!(
                "目标: {}（{} 模式）",
                chosen.join("+"),
                inputs.config.resolution.id()
            ));
            let before = state.active_sorted();
            let mut chosen_sorted = chosen.clone();
            chosen_sorted.sort();
            if before == chosen_sorted {
                lines.push("无需变更：当前已经是这个布局".to_string());
                return Outcome {
                    op: Op::Preview,
                    ok: true,
                    output: lines.join("\n"),
                    facts: Some(facts.clone()),
                    changed: false,
                };
            }
            match dbus::Conn::session()
                .map_err(|e| e.to_string())
                .and_then(|mut conn| {
                    mutter::apply(&mut conn, &state, logical, mutter::METHOD_VERIFY)
                        .map_err(|e| e.to_string())
                }) {
                Ok(()) => {
                    lines.push(
                        "verify: mutter 接受了这个布局（METHOD_VERIFY，没有生效）".to_string(),
                    );
                    lines.push(format!(
                        "实际仍是 {}，屏幕不会闪",
                        if before.is_empty() {
                            "无".to_string()
                        } else {
                            before.join("+")
                        }
                    ));
                    Outcome {
                        op: Op::Preview,
                        ok: true,
                        output: lines.join("\n"),
                        facts: Some(facts.clone()),
                        changed: false,
                    }
                }
                Err(error) => {
                    lines.push(format!("verify 失败：{error}"));
                    Outcome {
                        op: Op::Preview,
                        ok: false,
                        output: lines.join("\n"),
                        facts: Some(facts.clone()),
                        changed: false,
                    }
                }
            }
        }
    }
}

fn apply(inputs: &Inputs, facts: &StatusFacts) -> Outcome {
    let mut lines = vec!["$ gnous builtin displays / apply".to_string()];
    let (state, chosen, logical) = match plan_current(inputs) {
        Ok(plan) => plan,
        Err(error) => {
            lines.push(format!("无法应用：{error}"));
            return Outcome {
                op: Op::Apply,
                ok: false,
                output: lines.join("\n"),
                facts: Some(facts.clone()),
                changed: false,
            };
        }
    };
    lines.push(format!(
        "目标: {} · 分辨率 {}",
        chosen.join("+"),
        inputs.config.resolution.id()
    ));

    let before = state.active_sorted();
    let mut chosen_sorted = chosen.clone();
    chosen_sorted.sort();
    if before == chosen_sorted {
        lines.push("无需变更：当前已经是这个布局".to_string());
        return Outcome {
            op: Op::Apply,
            ok: true,
            output: lines.join("\n"),
            facts: Some(facts.clone()),
            changed: false,
        };
    }

    if config::dry_run() {
        lines.push("[dry-run] 会调用 ApplyMonitorsConfig(METHOD_PERSISTENT)".to_string());
        lines.push(format!(
            "[dry-run] 会从 {} 切到 {}",
            before.join("+"),
            chosen_sorted.join("+")
        ));
        return Outcome {
            op: Op::Apply,
            ok: true,
            output: lines.join("\n"),
            facts: Some(facts.clone()),
            changed: false,
        };
    }

    let mut conn = match dbus::Conn::session() {
        Ok(conn) => conn,
        Err(error) => {
            lines.push(format!("连不上会话总线：{error}"));
            return Outcome {
                op: Op::Apply,
                ok: false,
                output: lines.join("\n"),
                facts: Some(facts.clone()),
                changed: false,
            };
        }
    };

    // 先 verify：mutter 拒绝的布局绝不会被真的应用。
    if let Err(error) = mutter::apply(&mut conn, &state, logical.clone(), mutter::METHOD_VERIFY) {
        lines.push(format!("verify 失败，未改动显示：{error}"));
        return Outcome {
            op: Op::Apply,
            ok: false,
            output: lines.join("\n"),
            facts: Some(facts.clone()),
            changed: false,
        };
    }
    lines.push("verify: 通过".to_string());

    if let Err(error) = mutter::apply(&mut conn, &state, logical, mutter::METHOD_PERSISTENT) {
        lines.push(format!("apply 失败：{error}"));
        return Outcome {
            op: Op::Apply,
            ok: false,
            output: lines.join("\n"),
            facts: Some(facts.clone()),
            changed: false,
        };
    }

    // 用重新读到的真实状态确认，而不是相信刚才那次调用。
    std::thread::sleep(std::time::Duration::from_millis(350));
    let after = mutter::get_state(&mut conn).ok();
    let now = after
        .as_ref()
        .map(MutterState::active_sorted)
        .unwrap_or_default();
    let ok = now == chosen_sorted;
    lines.push(if ok {
        format!("已应用: {}", chosen_sorted.join("+"))
    } else {
        format!(
            "应用后状态不符：期望 {}，实际 {}",
            chosen_sorted.join("+"),
            if now.is_empty() {
                "无".to_string()
            } else {
                now.join("+")
            }
        )
    });
    let facts_after = (!ok).then(|| facts.clone());
    Outcome {
        op: Op::Apply,
        ok,
        output: lines.join("\n"),
        facts: facts_after,
        changed: ok,
    }
}

fn handover(inputs: &Inputs) -> Outcome {
    let mut lines = vec!["$ gnous builtin displays / handover".to_string()];
    lines.push("把显示控制权从外部脚本交给 GNOUS，只需要人手执行下面几步：".to_string());
    lines.push(String::new());
    for (index, command) in units::handover_commands(&inputs.config).iter().enumerate() {
        if command.starts_with('#') {
            lines.push(command.clone());
        } else {
            lines.push(format!("{}. {}", index + 1, command));
        }
    }
    lines.push(String::new());
    lines.push(format!(
        "当前: {} · {}",
        inputs.legacy_timer.one_line(),
        inputs.own_timer.one_line()
    ));
    lines.push("GNOUS 没有执行上面任何一条命令。".to_string());
    Outcome {
        op: Op::Handover,
        ok: true,
        output: lines.join("\n"),
        facts: None,
        changed: false,
    }
}

fn revert() -> Outcome {
    let mut lines = vec!["$ gnous builtin displays / revert".to_string()];
    match units::remove() {
        Ok(steps) => {
            lines.extend(steps);
            lines.push("配置与老脚本都保留，随时可以再 APPLY 一次。".to_string());
            Outcome {
                op: Op::Revert,
                ok: true,
                output: lines.join("\n"),
                facts: None,
                changed: true,
            }
        }
        Err(error) => {
            lines.push(format!("失败：{error}"));
            Outcome {
                op: Op::Revert,
                ok: false,
                output: lines.join("\n"),
                facts: None,
                changed: false,
            }
        }
    }
}

fn install_units(inputs: &Inputs) -> Outcome {
    let mut lines = Vec::new();
    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("gnous"));
    match units::install(&inputs.config, &exe) {
        Ok(steps) => {
            lines.extend(steps);
            Outcome {
                op: Op::Apply,
                ok: true,
                output: lines.join("\n"),
                facts: None,
                changed: true,
            }
        }
        Err(error) => {
            lines.push(format!("写单元失败：{error}"));
            Outcome {
                op: Op::Apply,
                ok: false,
                output: lines.join("\n"),
                facts: None,
                changed: false,
            }
        }
    }
}

pub fn run(op: Op) -> Outcome {
    let inputs = collect();
    let facts = build_facts(&inputs);
    match op {
        Op::Status => Outcome {
            op,
            ok: facts.bus_ok,
            output: status_text(&facts, &inputs),
            facts: Some(facts),
            changed: false,
        },
        Op::Doctor => Outcome {
            op,
            ok: facts.checks.iter().all(|c| c.ok),
            output: doctor_text(&facts),
            facts: Some(facts),
            changed: false,
        },
        Op::Preview => preview(&inputs, &facts),
        Op::Apply => {
            let mut outcome = apply(&inputs, &facts);
            // apply 成功后顺手把配置与 GNOUS 的单元文件按当前状态刷新一遍
            // （仍然不 enable、不 start）。
            if outcome.ok {
                if config::dry_run() {
                    outcome
                        .output
                        .push_str("\n[dry-run] 会写配置与 gnous-display.* 单元文件");
                } else {
                    match config::save(&inputs.config) {
                        Ok(path) => outcome
                            .output
                            .push_str(&format!("\n配置已写入 {}", path.display())),
                        Err(error) => outcome.output.push_str(&format!("\n配置写入失败：{error}")),
                    }
                    let installed = install_units(&inputs);
                    outcome.output.push('\n');
                    outcome.output.push_str(&installed.output);
                }
            }
            outcome
        }
        Op::Handover => handover(&inputs),
        Op::Revert => revert(),
    }
}

/// 只读的"当前应该是什么布局"，供 UI 直接展示。
pub fn snapshot() -> StatusFacts {
    build_facts(&collect())
}

/* ------------------------------- 定时器入口 -------------------------------- */

/// `gnous --builtin-display-auto`：GNOUS 自己的 timer 每隔几秒叫一次。
/// 只做一件事——把显示对账到配置描述的样子；已经一致就什么都不做。
pub fn auto_cli(args: &[String]) -> i32 {
    let mut verbose = false;
    for arg in args {
        match arg.as_str() {
            "--verbose" | "-v" => verbose = true,
            // 未知参数直接失败，避免拼错参数后静默什么都不做。
            other => {
                eprintln!("未知参数：{other}");
                return 2;
            }
        }
    }
    let (config, warning) = config::load();
    if let Some(warning) = &warning {
        eprintln!("{warning}");
    }
    if let Err(error) = config.validate() {
        eprintln!("配置不合法：{error}");
        return 2;
    }
    let inputs = collect();
    let facts = build_facts(&inputs);
    if !facts.drift {
        if verbose {
            println!("无需变更: {}", facts.active.join("+"));
        }
        return 0;
    }
    let outcome = apply(&inputs, &facts);
    println!("{}", outcome.output);
    if outcome.ok { 0 } else { 1 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::displays::mutter::{Logical, Mode as MutterMode};

    fn monitor(connector: &str, preferred: bool, current: bool) -> Monitor {
        Monitor {
            connector: connector.into(),
            vendor: "VND".into(),
            product: "PROD".into(),
            serial: "0".into(),
            modes: vec![
                MutterMode {
                    id: "2560x1440@60.000".into(),
                    width: 2560,
                    height: 1440,
                    rate: 60.0,
                    preferred,
                    current: allow_current(current, true),
                },
                MutterMode {
                    id: "1920x1080@60.000".into(),
                    width: 1920,
                    height: 1080,
                    rate: 60.0,
                    preferred: false,
                    current: false,
                },
            ],
        }
    }

    fn allow_current(current: bool, gate: bool) -> bool {
        current && gate
    }

    fn state(active: &str) -> MutterState {
        MutterState {
            serial: 7,
            monitors: vec![
                monitor("HDMI-1", true, active == "HDMI-1"),
                monitor("eDP-1", true, active == "eDP-1"),
            ],
            logical: vec![Logical {
                x: 0,
                y: 0,
                scale: 1.0,
                primary: true,
                connectors: vec![active.to_string()],
            }],
            layout_mode: Some(1),
        }
    }

    fn unit(present: bool, active: bool) -> units::UnitInfo {
        let mut info = units::UnitInfo {
            unit: "x.service".into(),
            ..units::UnitInfo::default()
        };
        info.properties.insert(
            "LoadState".into(),
            if present { "loaded" } else { "not-found" }.into(),
        );
        info.properties.insert(
            "ActiveState".into(),
            if active { "active" } else { "inactive" }.into(),
        );
        info
    }

    fn inputs(state: Result<MutterState, String>, legacy_active: bool) -> Inputs {
        Inputs {
            config: Config::default(),
            config_warning: None,
            connectors: vec![
                Connector {
                    name: "HDMI-1".into(),
                    status: "connected".into(),
                },
                Connector {
                    name: "eDP-1".into(),
                    status: "connected".into(),
                },
            ],
            state,
            legacy_timer: unit(true, legacy_active),
            legacy_service: unit(true, legacy_active),
            own_timer: unit(false, false),
        }
    }

    #[test]
    fn healthy_when_layout_matches_and_nothing_legacy_is_running() {
        let facts = build_facts(&inputs(Ok(state("HDMI-1")), false));
        assert_eq!(facts.headline, "healthy");
        assert!(!facts.drift);
        assert!(facts.checks.iter().all(|c| c.ok));
    }

    #[test]
    fn attention_when_the_legacy_script_still_reconciles() {
        let facts = build_facts(&inputs(Ok(state("HDMI-1")), true));
        assert_eq!(facts.headline, "attention");
        assert!(facts.headline_reason.contains("display-mode.service"));
        assert!(facts.config.takeover == false);
    }

    #[test]
    fn drift_is_reported_with_both_sides() {
        let facts = build_facts(&inputs(Ok(state("eDP-1")), true));
        assert!(facts.drift);
        assert_eq!(facts.active, vec!["eDP-1"]);
        assert_eq!(facts.desired, vec!["HDMI-1"]);
        let consistency = facts
            .checks
            .iter()
            .find(|c| c.name == "一致性")
            .expect("必须有一致性检查");
        assert!(!consistency.ok);
        assert!(consistency.detail.contains("HDMI-1"));
    }

    #[test]
    fn degraded_when_mutter_is_unreachable() {
        let facts = build_facts(&inputs(Err("连不上会话总线".into()), false));
        assert_eq!(facts.headline, "degraded");
        assert!(!facts.bus_ok);
        assert_eq!(facts.bus_error.as_deref(), Some("连不上会话总线"));
        // 期望布局仍然按 sysfs 推出来，方便解释"插着但看不见"。
        assert_eq!(facts.desired, vec!["HDMI-1"]);
    }

    #[test]
    fn auto_mode_falls_back_to_internal_when_external_is_unplugged() {
        let mut inputs = inputs(Ok(state("eDP-1")), false);
        inputs.connectors[0].status = "disconnected".into();
        let facts = build_facts(&inputs);
        assert_eq!(facts.desired, vec!["eDP-1"]);
        assert!(!facts.drift, "拔掉外接屏后内置屏就是对的目标");
    }

    #[test]
    fn resolution_strategy_changes_the_chosen_mode() {
        let state = state("HDMI-1");
        let hdmi = state.monitor("HDMI-1").unwrap();
        assert_eq!(
            pick_mode(hdmi, Resolution::Preferred).map(|m| m.id),
            Some("2560x1440@60.000".into())
        );
        assert_eq!(
            pick_mode(hdmi, Resolution::Max).map(|m| m.id),
            Some("2560x1440@60.000".into())
        );
        assert_eq!(
            pick_mode(hdmi, Resolution::Current).map(|m| m.id),
            Some("2560x1440@60.000".into())
        );
    }

    #[test]
    fn status_text_is_honest_about_a_missing_bus() {
        let inputs = inputs(Err("总线不存在".into()), false);
        let facts = build_facts(&inputs);
        let text = status_text(&facts, &inputs);
        assert!(text.contains("不可用"));
        assert!(text.contains("总线不存在"));
        assert!(text.contains("物理接口: HDMI-1=connected"));
    }

    #[test]
    fn every_op_has_a_distinct_id_and_label() {
        let mut ids: Vec<&str> = Op::ALL.iter().map(|op| op.id()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), Op::ALL.len());
        assert!(Op::Apply.is_mutating() && Op::Revert.is_mutating());
        assert!(Op::Preview.is_read_only() && Op::Status.is_read_only());
    }
}

/* ----------------------------------- UI ----------------------------------- */

/// 输出面板的标签页：一个 op 一个标签。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Status,
    Doctor,
    Preview,
    Apply,
    Handover,
    Revert,
}

impl Tab {
    pub const ALL: [Tab; 6] = [
        Tab::Status,
        Tab::Doctor,
        Tab::Preview,
        Tab::Apply,
        Tab::Handover,
        Tab::Revert,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Status => "STATUS",
            Tab::Doctor => "DOCTOR",
            Tab::Preview => "PREVIEW",
            Tab::Apply => "APPLY",
            Tab::Handover => "HANDOVER",
            Tab::Revert => "REVERT",
        }
    }

    pub fn op(self) -> Op {
        match self {
            Tab::Status => Op::Status,
            Tab::Doctor => Op::Doctor,
            Tab::Preview => Op::Preview,
            Tab::Apply => Op::Apply,
            Tab::Handover => Op::Handover,
            Tab::Revert => Op::Revert,
        }
    }

    fn of(op: Op) -> Tab {
        match op {
            Op::Status => Tab::Status,
            Op::Doctor => Tab::Doctor,
            Op::Preview => Tab::Preview,
            Op::Apply => Tab::Apply,
            Op::Handover => Tab::Handover,
            Op::Revert => Tab::Revert,
        }
    }
}

/// UI 状态，由 `App` 持有。
#[derive(Clone, Debug)]
pub struct State {
    pub tab: Tab,
    pub busy: bool,
    /// 需要第二次点击确认的 op。
    pub armed: Option<Op>,
    /// 每个 op 最近一次的结果。用 Vec 是因为 `Op` 不是密集索引。
    pub outcomes: Vec<(Op, Outcome)>,
    pub facts: Option<StatusFacts>,
    /// 左列高亮的面。
    pub face: usize,
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
