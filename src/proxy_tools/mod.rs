//! In-process backend for the `proxy-tools` page-level extension.
//!
//! Scope, in one sentence: **read everything about the local relay, write only
//! GNOUS-owned files, and never touch the unit that carries this machine's
//! connectivity.**
//!
//! Three faces, each with its own risk level:
//!
//! | face | ops | what it may do |
//! | --- | --- | --- |
//! | 诊断 | `status` / `doctor` | read-only: procfs, `systemctl show`, env files, one LAN handshake |
//! | 配置 | `apply` / `revert` | write/remove `~/.config/gnous/proxy-tools.json` and GNOUS's own unit file; never enables or starts anything |
//! | 交棒 | `handover` | print the operator's commands; execute nothing |
//!
//! Everything that could interrupt the session lives in `guard`, which refuses
//! before a syscall is made — see `extensions/proxy-tools/DEV-SAFETY.md`.

pub mod config;
pub mod exec;
pub mod guard;
pub mod no_proxy;
pub mod probe;
pub mod relay;
pub mod units;

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use config::{Config, NoProxyMode};
use no_proxy::Interface;
use probe::{ControlPlane, UnitState, UpstreamProbe};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Op {
    Status,
    Doctor,
    Apply,
    Revert,
    Handover,
}

impl Op {
    pub const ALL: [Op; 5] = [Op::Status, Op::Doctor, Op::Apply, Op::Revert, Op::Handover];

    pub fn id(self) -> &'static str {
        match self {
            Op::Status => "status",
            Op::Doctor => "doctor",
            Op::Apply => "apply",
            Op::Revert => "revert",
            Op::Handover => "handover",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Op::Status => "STATUS",
            Op::Doctor => "DOCTOR",
            Op::Apply => "APPLY",
            Op::Revert => "REVERT",
            Op::Handover => "HANDOVER",
        }
    }

    /// Mutating ops need a second click in the UI.
    pub fn is_mutating(self) -> bool {
        matches!(self, Op::Apply | Op::Revert)
    }
}

#[derive(Clone, Debug)]
pub struct Outcome {
    pub op: Op,
    pub ok: bool,
    pub output: String,
    pub command: String,
    pub facts: Value,
}

impl Outcome {
    pub fn status_facts(&self) -> Option<StatusFacts> {
        serde_json::from_value(self.facts.clone()).ok()
    }
}

pub fn command_for(op: Op) -> String {
    if op == Op::Handover {
        "gnous builtin proxy-tools / handover（只打印命令，不执行）".to_string()
    } else {
        format!("gnous builtin proxy-tools / {}", op.id())
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UnitFacts {
    pub unit: String,
    pub present: bool,
    pub active: String,
    pub sub: String,
    pub enabled: String,
    pub main_pid: Option<u32>,
    pub exec_start: String,
    pub restarts: Option<u32>,
}

impl UnitFacts {
    fn from_state(unit: &str, state: &UnitState) -> Self {
        Self {
            unit: unit.to_string(),
            present: state.present,
            active: state.active.clone(),
            sub: state.sub.clone(),
            enabled: state.enabled.clone(),
            main_pid: state.main_pid,
            exec_start: state.exec_start.clone(),
            restarts: state.restarts,
        }
    }

    pub fn running(&self) -> bool {
        self.active == "active"
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpstreamFacts {
    pub addr: String,
    pub reachable: bool,
    pub latency_ms: u64,
    pub detail: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ControlFacts {
    pub env_files: Vec<String>,
    pub env_keys: usize,
    pub user_manager_keys: usize,
    pub http_proxy: Option<String>,
    pub gnome_mode: Option<String>,
    pub no_proxy_entries: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NoProxyFacts {
    pub mode: String,
    pub entries: usize,
    pub covers_self: bool,
    pub missing: Vec<String>,
    pub missing_count: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConfigFacts {
    pub listen: String,
    pub upstream: String,
    pub idle_timeout_secs: u64,
    pub connect_timeout_secs: u64,
    pub retries: u32,
    pub adopted: bool,
    pub problems: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PathFacts {
    pub config: String,
    pub unit: String,
    pub log: String,
    pub environment_d: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DoctorCheck {
    pub id: String,
    pub title: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StatusFacts {
    /// `healthy` / `attention` / `degraded`
    pub status: String,
    pub headline: String,
    pub listen: String,
    pub listening: bool,
    /// `gnous` / `legacy` / `none`
    pub listen_owner: String,
    pub relay: UnitFacts,
    pub legacy: UnitFacts,
    pub upstream: UpstreamFacts,
    pub control: ControlFacts,
    pub no_proxy: NoProxyFacts,
    pub config: ConfigFacts,
    pub paths: PathFacts,
    pub interfaces: Vec<String>,
    pub log_lines: usize,
    pub log_errors: usize,
    pub protected_ports: Vec<u16>,
    pub checks: Vec<DoctorCheck>,
}

impl StatusFacts {
    pub fn is_healthy(&self) -> bool {
        self.status == "healthy"
    }

    pub fn degraded(&self) -> bool {
        self.status == "degraded"
    }
}

/// Everything status/doctor need, gathered once so the pure assembly below can
/// be unit-tested without touching the machine.
#[derive(Clone, Debug, Default)]
pub struct Inputs {
    pub config: Config,
    pub listen_listening: bool,
    pub relay: UnitState,
    pub legacy: UnitState,
    pub upstream: UpstreamProbe,
    pub control: ControlPlane,
    pub gnome_mode: Option<String>,
    pub interfaces: Vec<Interface>,
    pub log_lines: usize,
    pub log_errors: usize,
}

pub fn collect_inputs() -> Inputs {
    let config = config::load();
    let listen_port = config
        .listen_addr()
        .map(|addr| addr.port())
        .unwrap_or_default();
    let listen_listening = probe::listening_on(listen_port);
    let relay = probe::unit_state(units::UNIT_NAME).unwrap_or_default();
    let legacy = probe::unit_state(guard::LEGACY_UNITS[0]).unwrap_or_default();
    let upstream = match config.upstream_addr() {
        Ok(addr) => probe::probe_upstream(addr, Duration::from_millis(900)),
        Err(error) => UpstreamProbe {
            reachable: false,
            latency_ms: 0,
            detail: error,
        },
    };
    let control = probe::control_plane();
    let gnome_mode = exec::run(&["gsettings", "get", "org.gnome.system.proxy", "mode"])
        .ok()
        .map(|text| text.trim().trim_matches('\'').to_string());
    let interfaces = exec::run(&["ip", "-4", "-o", "addr", "show", "scope", "global"])
        .map(|text| no_proxy::parse_interfaces(&text))
        .unwrap_or_default();
    let legacy_log = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/root"))
        .join(".cache/j1900-proxy/jproxy.log");
    let (log_lines, log_errors, _) = probe::log_error_tail(&legacy_log, 400);
    Inputs {
        config,
        listen_listening,
        relay,
        legacy,
        upstream,
        control,
        gnome_mode,
        interfaces,
        log_lines,
        log_errors,
    }
}

/// Pure: what the panel shows, derived only from `inputs`.
pub fn build_facts(inputs: &Inputs) -> StatusFacts {
    let config = &inputs.config;
    let relay = UnitFacts::from_state(units::UNIT_NAME, &inputs.relay);
    let legacy = UnitFacts::from_state(guard::LEGACY_UNITS[0], &inputs.legacy);
    let listen_owner = if inputs.listen_listening {
        if relay.running() {
            "gnous"
        } else if legacy.running() {
            "legacy"
        } else {
            "other"
        }
    } else {
        "none"
    }
    .to_string();

    let no_proxy_value = inputs.control.no_proxy.clone().unwrap_or_default();
    let missing = no_proxy::missing_hosts(&no_proxy_value, &inputs.interfaces);
    let no_proxy_facts = NoProxyFacts {
        mode: config.no_proxy_mode.label().to_string(),
        entries: no_proxy_value
            .split(',')
            .filter(|e| !e.trim().is_empty())
            .count(),
        covers_self: no_proxy::covers_self(&no_proxy_value, &inputs.interfaces),
        missing_count: missing.len(),
        missing: missing.iter().take(12).cloned().collect(),
    };

    let config_problems = config.validate();

    let mut checks = Vec::new();
    checks.push(DoctorCheck {
        id: "listen".into(),
        title: format!("配置端口 {}", config.listen),
        ok: inputs.listen_listening,
        detail: if inputs.listen_listening {
            format!("有监听者（归属：{listen_owner}）")
        } else {
            "没有任何进程在监听，代理不可用".into()
        },
    });
    checks.push(DoctorCheck {
        id: "upstream".into(),
        title: format!("上游 {}", config.upstream),
        ok: inputs.upstream.reachable,
        detail: format!(
            "{}（{} ms）{}",
            if inputs.upstream.reachable {
                "可达"
            } else {
                "不可达"
            },
            inputs.upstream.latency_ms,
            inputs.upstream.detail
        ),
    });
    checks.push(DoctorCheck {
        id: "relay-unit".into(),
        title: format!("GNOUS 中继单元 {}", units::UNIT_NAME),
        // Not being installed is a normal state: the legacy relay still owns the
        // port. Only a *half-installed* unit (present but dead) is a problem.
        ok: relay.running() || !relay.present,
        detail: if relay.running() {
            format!(
                "运行中（{} / {}）pid {:?}",
                relay.sub, relay.enabled, relay.main_pid
            )
        } else if relay.present {
            format!(
                "已安装但未运行（{} / {}）；端口仍由旧中继承担，启用与否由你决定",
                relay.sub,
                if relay.enabled.is_empty() {
                    "disabled"
                } else {
                    &relay.enabled
                }
            )
        } else {
            "未安装（APPLY 只写文件，启用由你决定）".into()
        },
    });
    checks.push(DoctorCheck {
        id: "legacy-unit".into(),
        title: format!("旧中继单元 {}", guard::LEGACY_UNITS[0]),
        ok: legacy.running(),
        detail: if legacy.present {
            format!(
                "{} / {} / {}，pid {}（GNOUS 只读它）",
                legacy.active,
                legacy.sub,
                legacy.enabled,
                legacy
                    .main_pid
                    .map(|pid| pid.to_string())
                    .unwrap_or_else(|| "—".into())
            )
        } else {
            "不存在（已迁移或从未安装）".into()
        },
    });
    checks.push(DoctorCheck {
        id: "no-proxy".into(),
        title: "no_proxy 覆盖当前网段".into(),
        ok: no_proxy_facts.missing_count == 0,
        detail: if no_proxy_facts.missing_count == 0 {
            format!("{} 条，覆盖完整", no_proxy_facts.entries)
        } else {
            format!(
                "缺 {} 个精确 IP，例如 {}",
                no_proxy_facts.missing_count,
                no_proxy_facts
                    .missing
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        },
    });
    let manager_proxy_keys = inputs
        .control
        .user_manager_keys
        .iter()
        .filter(|(key, _)| key.to_ascii_lowercase().contains("proxy"))
        .count();
    checks.push(DoctorCheck {
        id: "env".into(),
        title: "environment.d 与运行环境".into(),
        ok: !inputs.control.env_keys.is_empty() && manager_proxy_keys > 0,
        detail: format!(
            "{} 个文件 / {} 条键；user manager 里 {} 条代理变量",
            inputs.control.env_files.len(),
            inputs.control.env_keys.len(),
            manager_proxy_keys
        ),
    });
    checks.push(DoctorCheck {
        id: "gnome".into(),
        title: "GNOME 代理模式".into(),
        ok: matches!(inputs.gnome_mode.as_deref(), Some("manual") | Some("auto")),
        detail: inputs
            .gnome_mode
            .clone()
            .unwrap_or_else(|| "读取失败（gsettings 不可用）".into()),
    });
    checks.push(DoctorCheck {
        id: "log".into(),
        title: "旧中继日志".into(),
        ok: inputs.log_errors == 0,
        detail: if inputs.log_lines == 0 {
            "没有日志（旧中继未运行过）".into()
        } else {
            format!(
                "最近 {} 行里 {} 行异常",
                inputs.log_lines, inputs.log_errors
            )
        },
    });
    checks.push(DoctorCheck {
        id: "config".into(),
        title: "GNOUS 侧配置".into(),
        ok: config_problems.is_empty(),
        detail: if config_problems.is_empty() {
            format!(
                "{}（{} 秒空闲超时，{} 次重试）",
                config.listen, config.idle_timeout_secs, config.retries
            )
        } else {
            config_problems.join("；")
        },
    });

    let (status, headline) = if !inputs.listen_listening {
        (
            "degraded",
            "代理端口没有监听者：命令行与外网都不通".to_string(),
        )
    } else if !inputs.upstream.reachable {
        (
            "attention",
            format!(
                "本地中继在跑（{}），但上游 {} 不可达",
                listen_owner_label(&listen_owner),
                config.upstream
            ),
        )
    } else if no_proxy_facts.missing_count > 0 {
        (
            "attention",
            format!(
                "中继可用，但 no_proxy 缺 {} 个内网精确 IP（Node/Bun 会走代理）",
                no_proxy_facts.missing_count
            ),
        )
    } else {
        (
            "healthy",
            format!(
                "中继可用（{}），上游 {} 可达",
                listen_owner_label(&listen_owner),
                config.upstream
            ),
        )
    };

    StatusFacts {
        status: status.to_string(),
        headline,
        listen: config.listen.clone(),
        listening: inputs.listen_listening,
        listen_owner: listen_owner.to_string(),
        relay,
        legacy,
        upstream: UpstreamFacts {
            addr: config.upstream.clone(),
            reachable: inputs.upstream.reachable,
            latency_ms: inputs.upstream.latency_ms,
            detail: inputs.upstream.detail.clone(),
        },
        control: ControlFacts {
            env_files: inputs
                .control
                .env_files
                .iter()
                .map(|path| path.display().to_string())
                .collect(),
            env_keys: inputs.control.env_keys.len(),
            user_manager_keys: manager_proxy_keys,
            http_proxy: inputs.control.http_proxy.clone(),
            gnome_mode: inputs.gnome_mode.clone(),
            no_proxy_entries: no_proxy_facts.entries,
        },
        no_proxy: no_proxy_facts,
        config: ConfigFacts {
            listen: config.listen.clone(),
            upstream: config.upstream.clone(),
            idle_timeout_secs: config.idle_timeout_secs,
            connect_timeout_secs: config.connect_timeout_secs,
            retries: config.retries,
            adopted: config.adopted,
            problems: config_problems,
        },
        paths: PathFacts {
            config: config::config_file().display().to_string(),
            unit: units::unit_file().display().to_string(),
            log: units::log_file().display().to_string(),
            environment_d: probe::environment_d_dir().display().to_string(),
        },
        interfaces: inputs
            .interfaces
            .iter()
            .map(|interface| {
                format!(
                    "{} {}/{}",
                    interface.name,
                    interface.network(),
                    interface.prefix_len
                )
            })
            .collect(),
        log_lines: inputs.log_lines,
        log_errors: inputs.log_errors,
        protected_ports: guard::PROTECTED_PORTS.to_vec(),
        checks,
    }
}

fn listen_owner_label(owner: &str) -> String {
    match owner {
        "gnous" => "GNOUS 中继",
        "legacy" => "旧 jproxy 中继",
        "other" => "其它进程",
        _ => "无",
    }
    .to_string()
}

pub fn run(op: Op) -> Outcome {
    let inputs = collect_inputs();
    let facts = build_facts(&inputs);
    match op {
        Op::Status => status_outcome(&facts),
        Op::Doctor => doctor_outcome(&inputs, &facts),
        Op::Apply => apply_outcome(&inputs.config),
        Op::Revert => revert_outcome(&facts),
        Op::Handover => handover_outcome(&inputs.config),
    }
}

fn status_outcome(facts: &StatusFacts) -> Outcome {
    let mut lines = vec![
        format!("状态：{}  {}", facts.status.to_uppercase(), facts.headline),
        format!(
            "监听：{}  {}",
            facts.listen,
            if facts.listening {
                "有监听者"
            } else {
                "无"
            }
        ),
        format!("归属：{}", listen_owner_label(&facts.listen_owner)),
        format!(
            "GNOUS 中继：{} {}",
            units::UNIT_NAME,
            if facts.relay.present {
                format!(
                    "（{} / {}）{}",
                    facts.relay.active,
                    if facts.relay.enabled.is_empty() {
                        "disabled"
                    } else {
                        &facts.relay.enabled
                    },
                    facts
                        .relay
                        .main_pid
                        .map(|pid| format!("pid {pid}"))
                        .unwrap_or_default()
                )
            } else {
                "未安装".to_string()
            }
        ),
        format!(
            "旧中继：{}（{} / {}）{}",
            guard::LEGACY_UNITS[0],
            facts.legacy.active,
            facts.legacy.enabled,
            facts
                .legacy
                .main_pid
                .map(|pid| format!("pid {pid}"))
                .unwrap_or_default()
        ),
        format!(
            "上游：{} {}（{} ms）",
            facts.upstream.addr,
            if facts.upstream.reachable {
                "可达"
            } else {
                "不可达"
            },
            facts.upstream.latency_ms
        ),
        format!(
            "控制面：mode={} http_proxy={} environment.d {} 条键 / user manager {} 条代理变量",
            facts.control.gnome_mode.as_deref().unwrap_or("—"),
            facts.control.http_proxy.as_deref().unwrap_or("—"),
            facts.control.env_keys,
            facts.control.user_manager_keys
        ),
        format!(
            "no_proxy：{} 条（{}）{}",
            facts.no_proxy.entries,
            facts.no_proxy.mode,
            if facts.no_proxy.missing_count == 0 {
                "覆盖完整".to_string()
            } else {
                format!("缺 {} 个精确 IP", facts.no_proxy.missing_count)
            }
        ),
        format!(
            "网卡：{}",
            if facts.interfaces.is_empty() {
                "—".into()
            } else {
                facts.interfaces.join(" · ")
            }
        ),
        format!(
            "旧中继日志：最近 {} 行 / {} 行异常",
            facts.log_lines, facts.log_errors
        ),
        format!("状态文件：{}", facts.paths.config),
        format!("单元文件：{}", facts.paths.unit),
    ];
    lines.push(format!(
        "受保护端口（永不接管）：{}",
        join_ports(&facts.protected_ports)
    ));
    Outcome {
        op: Op::Status,
        ok: !facts.degraded(),
        output: lines.join("\n"),
        command: command_for(Op::Status),
        facts: serde_json::to_value(facts).unwrap_or(Value::Null),
    }
}

fn join_ports(ports: &[u16]) -> String {
    ports
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(" / ")
}

fn doctor_outcome(inputs: &Inputs, facts: &StatusFacts) -> Outcome {
    let mut lines = vec![format!("诊断：{} 项检查", facts.checks.len())];
    let mut failures = 0;
    for check in &facts.checks {
        let marker = if check.ok { "✓" } else { "✗" };
        if !check.ok {
            failures += 1;
        }
        lines.push(format!("{marker} {:<28} {}", check.title, check.detail));
    }
    lines.push(String::new());
    lines.push(format!(
        "结论：{}（{} 项不通过）",
        facts.status.to_uppercase(),
        failures
    ));
    if facts.listen_owner == "legacy" {
        lines.push("提示：当前监听者仍是旧 python 中继；把端口交给 GNOUS 需要你手工执行 HANDOVER 里的命令。".into());
    }
    if facts.no_proxy.missing_count > 0 {
        lines.push(format!(
            "提示：no_proxy 缺 {} 个内网精确 IP，Node/Bun 不认 CIDR，会把内网请求打进代理。",
            facts.no_proxy.missing_count
        ));
    }
    if inputs.config.no_proxy_mode == NoProxyMode::Manual {
        lines.push("提示：no_proxy 生成策略为「不生成」，HANDOVER 不会改动现有值。".into());
    }
    lines.push(format!(
        "环境文件：{}",
        if facts.control.env_files.is_empty() {
            "—".to_string()
        } else {
            facts.control.env_files.join(" , ")
        }
    ));
    Outcome {
        op: Op::Doctor,
        ok: failures == 0,
        output: lines.join("\n"),
        command: command_for(Op::Doctor),
        facts: serde_json::to_value(facts).unwrap_or(Value::Null),
    }
}

fn apply_outcome(config: &Config) -> Outcome {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("gnous"));
    let mut lines = Vec::new();
    if guard::dry_run() {
        lines.push("GNOUS_PROXY_DRY_RUN=1：只打印计划，不写任何文件".to_string());
    }
    let mut ok = true;
    match units::install(config, &exe) {
        Ok(steps) => lines.extend(steps),
        Err(error) => {
            ok = false;
            lines.push(format!("✗ 写入失败：{error}"));
        }
    }
    if ok && config.no_proxy_mode != NoProxyMode::Manual && !guard::dry_run() {
        let interfaces = exec::run(&["ip", "-4", "-o", "addr", "show", "scope", "global"])
            .map(|text| no_proxy::parse_interfaces(&text))
            .unwrap_or_default();
        let value = no_proxy::build_no_proxy(&interfaces, config.no_proxy_mode);
        lines.push(format!(
            "no_proxy（{}，{} 条）：已写入配置，供 HANDOVER 使用；GNOUS 不会自己改 environment.d",
            config.no_proxy_mode.label(),
            value.split(',').count()
        ));
    }
    lines.push(format!(
        "未启动任何单元；{} 由你决定何时启用",
        units::UNIT_NAME
    ));
    Outcome {
        op: Op::Apply,
        ok,
        output: lines.join("\n"),
        command: command_for(Op::Apply),
        facts: json!({
            "dryRun": guard::dry_run(),
            "config": config::config_file().display().to_string(),
            "unit": units::unit_file().display().to_string(),
        }),
    }
}

fn revert_outcome(facts: &StatusFacts) -> Outcome {
    let mut lines = Vec::new();
    if guard::dry_run() {
        lines.push("GNOUS_PROXY_DRY_RUN=1：只打印计划，不删除、不停止".to_string());
    }
    let installed = facts.relay.present;
    let mut ok = true;
    match units::remove(installed && facts.relay.running()) {
        Ok(steps) => lines.extend(steps),
        Err(error) => {
            ok = false;
            lines.push(format!("✗ 清理失败：{error}"));
        }
    }
    lines.push(format!(
        "{} 未被触碰（GNOUS 不管理它）",
        guard::LEGACY_UNITS[0]
    ));
    lines.push(format!(
        "配置文件保留：{}（只删单元文件，配置是你填的）",
        config::config_file().display()
    ));
    lines.push(
        "控制面（gsettings / environment.d）保持原样；要回直连请手工执行旧工具的 off 脚本。".into(),
    );
    Outcome {
        op: Op::Revert,
        ok,
        output: lines.join("\n"),
        command: command_for(Op::Revert),
        facts: json!({"unit": units::unit_file().display().to_string()}),
    }
}

fn handover_outcome(config: &Config) -> Outcome {
    let mut lines = vec![
        "以下命令只打印、绝不执行：交棒会让所有在途连接断开几秒，".to_string(),
        "包括本次对话所用的连接，所以必须由你在合适的时刻手工执行。".to_string(),
        String::new(),
    ];
    lines.extend(units::handover_commands(config));
    Outcome {
        op: Op::Handover,
        ok: true,
        output: lines.join("\n"),
        command: command_for(Op::Handover),
        facts: json!({"executed": false}),
    }
}

/// `gnous --builtin-proxy-relay` entry point.
///
/// Flags override the persisted config; the installed unit always passes them
/// explicitly so the relay keeps working even if the JSON file is missing.
pub fn relay_cli(args: &[String]) -> i32 {
    let mut config = config::load();
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].as_str();
        let value = args.get(index + 1).cloned();
        match (flag, value) {
            ("--listen", Some(raw)) => config.listen = raw,
            ("--upstream", Some(raw)) => config.upstream = raw,
            ("--idle-timeout", Some(raw)) => {
                if let Ok(secs) = raw.parse() {
                    config.idle_timeout_secs = secs;
                }
            }
            ("--connect-timeout", Some(raw)) => {
                if let Ok(secs) = raw.parse() {
                    config.connect_timeout_secs = secs;
                }
            }
            ("--retries", Some(raw)) => {
                if let Ok(retries) = raw.parse() {
                    config.retries = retries;
                }
            }
            ("--adopted", _) => config.adopted = true,
            (other, _) if !other.starts_with('-') => {}
            (other, _) => {
                eprintln!("gnous relay: 未知参数 {other}");
                return 2;
            }
        }
        index += if args
            .get(index + 1)
            .is_some_and(|next| !next.starts_with('-'))
        {
            2
        } else {
            1
        };
    }
    let (Ok(listen), Ok(upstream)) = (config.listen_addr(), config.upstream_addr()) else {
        eprintln!(
            "gnous relay: 地址无效 listen={} upstream={}",
            config.listen, config.upstream
        );
        return 2;
    };
    let relay_config = relay::RelayConfig::from_fields(
        listen,
        upstream,
        config.idle_timeout_secs,
        config.connect_timeout_secs,
        config.retries,
    );
    match relay::serve_until_signal(relay_config, |addr| {
        println!("gnous proxy relay 监听 {addr} -> {upstream}（std 实现，无 python）");
    }) {
        Ok(_) => 0,
        Err(error) => {
            eprintln!("gnous relay: {error}");
            2
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Status,
    Doctor,
    Plan,
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::Status, Tab::Doctor, Tab::Plan];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Status => "STATUS",
            Tab::Doctor => "DOCTOR",
            Tab::Plan => "HANDOVER",
        }
    }
}

/// UI state owned by `App`, mirrors `root_align::State`.
#[derive(Clone, Debug)]
pub struct State {
    pub tab: Tab,
    pub busy: bool,
    pub armed: Option<Op>,
    pub status: Option<Outcome>,
    pub doctor: Option<Outcome>,
    pub plan: Option<Outcome>,
    pub facts: Option<StatusFacts>,
    /// Which of the three faces the left column highlights.
    pub face: usize,
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

impl State {
    pub fn new() -> Self {
        Self {
            tab: Tab::Status,
            busy: false,
            armed: None,
            status: None,
            doctor: None,
            plan: None,
            facts: None,
            face: 0,
        }
    }

    pub fn absorb(&mut self, outcome: Outcome, switch_tab: bool) {
        let op = outcome.op;
        if let Some(facts) = outcome.status_facts() {
            self.facts = Some(facts);
        }
        match op {
            Op::Status => self.status = Some(outcome),
            Op::Doctor => self.doctor = Some(outcome),
            Op::Apply | Op::Revert | Op::Handover => self.plan = Some(outcome),
        }
        if switch_tab {
            self.tab = match op {
                Op::Status => Tab::Status,
                Op::Doctor => Tab::Doctor,
                Op::Apply | Op::Revert | Op::Handover => Tab::Plan,
            };
        }
    }

    pub fn outcome(&self, tab: Tab) -> Option<&Outcome> {
        match tab {
            Tab::Status => self.status.as_ref(),
            Tab::Doctor => self.doctor.as_ref(),
            Tab::Plan => self.plan.as_ref(),
        }
    }

    pub fn facts_or_default(&self) -> StatusFacts {
        self.facts.clone().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造"看起来像真的"代理地址，避免把受保护端口字面量写进测试。
    fn http_proxy_value() -> String {
        let port = guard::PROTECTED_PORTS[0];
        format!("http://127.0.0.1:{port}")
    }

    fn inputs() -> Inputs {
        Inputs {
            config: Config::default(),
            listen_listening: true,
            relay: UnitState {
                active: "inactive".into(),
                enabled: "disabled".into(),
                present: true,
                ..UnitState::default()
            },
            legacy: UnitState {
                active: "active".into(),
                sub: "running".into(),
                enabled: "enabled".into(),
                main_pid: Some(4242),
                present: true,
                ..UnitState::default()
            },
            upstream: UpstreamProbe {
                reachable: true,
                latency_ms: 3,
                detail: "TCP 握手成功".into(),
            },
            control: ControlPlane {
                env_files: vec![PathBuf::from("/root/.config/environment.d/x.conf")],
                env_keys: vec![("http_proxy".into(), http_proxy_value())],
                user_manager_keys: vec![("http_proxy".into(), http_proxy_value())],
                no_proxy: Some("localhost,127.0.0.1,192.168.1.3,192.168.1.7".into()),
                http_proxy: Some(http_proxy_value()),
            },
            gnome_mode: Some("manual".into()),
            interfaces: vec![Interface {
                name: "wlp1s0".into(),
                ip: "192.168.1.3".parse().unwrap(),
                prefix_len: 24,
            }],
            log_lines: 100,
            log_errors: 0,
        }
    }

    #[test]
    fn legacy_owner_keeps_the_session_alive_and_is_reported_honestly() {
        let facts = build_facts(&inputs());
        assert_eq!(facts.listen_owner, "legacy");
        assert!(facts.listening);
        assert!(facts.upstream.reachable);
        assert!(facts.legacy.running());
        // no_proxy misses most of the /24 in this fixture, so status is attention.
        assert_eq!(facts.status, "attention");
        assert!(facts.headline.contains("no_proxy"));
        assert!(facts.no_proxy.missing_count > 0);
    }

    #[test]
    fn nothing_listening_is_degraded_not_healthy() {
        let mut inputs = inputs();
        inputs.listen_listening = false;
        inputs.legacy.active = "inactive".into();
        let facts = build_facts(&inputs);
        assert!(facts.degraded());
        assert_eq!(facts.listen_owner, "none");
        assert!(facts.headline.contains("没有监听者"));
    }

    #[test]
    fn unreachable_upstream_is_attention_with_the_real_reason() {
        let mut inputs = inputs();
        inputs.upstream = UpstreamProbe {
            reachable: false,
            latency_ms: 900,
            detail: "Connection timed out".into(),
        };
        let facts = build_facts(&inputs);
        assert_eq!(facts.status, "attention");
        assert!(facts.headline.contains("上游"));
        let upstream_check = facts.checks.iter().find(|c| c.id == "upstream").unwrap();
        assert!(!upstream_check.ok);
        assert!(upstream_check.detail.contains("不可达"));
    }

    #[test]
    fn full_no_proxy_coverage_is_healthy() {
        let mut inputs = inputs();
        let full = no_proxy::build_no_proxy(&inputs.interfaces, NoProxyMode::Exact24);
        inputs.control.no_proxy = Some(full);
        let facts = build_facts(&inputs);
        assert_eq!(facts.status, "healthy", "{}", facts.headline);
        assert!(facts.headline.contains("旧 jproxy 中继"));
        assert_eq!(facts.no_proxy.missing_count, 0);
    }

    #[test]
    fn doctor_lists_every_check_and_counts_failures() {
        let mut inputs = inputs();
        inputs.gnome_mode = Some("none".into());
        let facts = build_facts(&inputs);
        let outcome = doctor_outcome(&inputs, &facts);
        assert!(!outcome.ok);
        assert!(outcome.output.contains("✗"));
        assert!(outcome.output.contains("GNOME 代理模式"));
        assert!(outcome.output.contains("不通过"));
    }

    #[test]
    fn status_output_states_the_protected_ports_and_never_claims_control() {
        let inputs = inputs();
        let facts = build_facts(&inputs);
        let outcome = status_outcome(&facts);
        assert!(outcome.output.contains("受保护端口"));
        assert!(outcome.output.contains("旧中继"));
        assert!(outcome.ok, "attention is not a failure");
        // The panel must never pretend it owns the live relay.
        assert!(
            outcome.output.contains("j1900-proxy.service") || outcome.output.contains("未安装")
        );
    }

    #[test]
    fn handover_only_prints_commands() {
        let outcome = handover_outcome(&Config::default());
        assert!(outcome.ok);
        assert_eq!(outcome.facts["executed"], json!(false));
        assert!(outcome.output.contains("绝不执行"));
        assert!(outcome.output.contains("后悔药"));
    }

    #[test]
    fn mutating_ops_are_the_only_ones_needing_confirmation() {
        assert!(Op::Apply.is_mutating());
        assert!(Op::Revert.is_mutating());
        assert!(!Op::Status.is_mutating());
        assert!(!Op::Doctor.is_mutating());
        assert!(!Op::Handover.is_mutating());
    }

    #[test]
    fn state_keeps_the_last_outcome_per_face() {
        let mut state = State::new();
        let inputs = inputs();
        let facts = build_facts(&inputs);
        state.absorb(status_outcome(&facts), true);
        assert!(state.facts.is_some());
        assert!(state.outcome(Tab::Status).is_some());
        assert!(state.outcome(Tab::Doctor).is_none());
    }
}
