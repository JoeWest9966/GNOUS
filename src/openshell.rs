//! Data layer for the `openshell` page-level extension.
//!
//! Every value shown by the panel comes from one of the commands below, run
//! verbatim. Parsing is deliberately loose: unknown or changed output falls
//! back to the raw text instead of inventing state (`docs/EXTENSIONS.md`,
//! `extensions/root-align/README.md`).

use std::process::Command;

use serde_json::Value;

/// CLI binary. Resolved through `PATH`; a missing binary becomes a visible
/// failure, never a fabricated success.
pub const BIN: &str = "openshell";

/// Timeout wrapper so a stuck gateway cannot freeze the panel.
const TIMEOUT_BIN: &str = "timeout";

// ============================================================================
// Command plumbing
// ============================================================================

#[derive(Clone, Debug, Default)]
pub struct CmdOut {
    pub ok: bool,
    pub text: String,
}

impl CmdOut {
    pub fn failure(reason: impl Into<String>) -> Self {
        Self {
            ok: false,
            text: reason.into(),
        }
    }

    /// First line that carries information, for dense summaries.
    pub fn headline(&self) -> String {
        self.text
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or("（无输出）")
            .to_string()
    }

    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }
}

/// Run `openshell <argv>` through `timeout`. Never panics, never blocks longer
/// than `secs`.
pub fn cli(argv: &[&str], secs: u64) -> CmdOut {
    let mut command = Command::new(TIMEOUT_BIN);
    command.arg(secs.to_string()).arg(BIN).args(argv);
    match command.output() {
        Ok(out) => {
            let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
            let err = String::from_utf8_lossy(&out.stderr);
            if !err.trim().is_empty() {
                if !text.trim().is_empty() {
                    text.push('\n');
                }
                text.push_str(err.trim_end());
            }
            CmdOut {
                ok: out.status.success(),
                text,
            }
        }
        Err(error) => CmdOut::failure(format!(
            "无法启动 `{BIN} {}`: {error}\n面板不会伪造状态，请确认 openshell 在 PATH 中。",
            argv.join(" ")
        )),
    }
}

/// Same as [`cli`], but `--output json` is appended.
pub fn cli_json(argv: &[&str], secs: u64) -> CmdOut {
    let mut args: Vec<&str> = argv.to_vec();
    args.extend(["-o", "json"]);
    cli(&args, secs)
}

// ============================================================================
// Snapshot
// ============================================================================

/// One probe of the gateway. Read-only; safe to run on every page entry.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub probed_at: String,
    pub status: CmdOut,
    pub whoami: CmdOut,
    pub gateway_list: CmdOut,
    pub gateway_info: CmdOut,
    pub doctor: CmdOut,
    pub sandbox_list: CmdOut,
    pub template_list: CmdOut,
    pub provider_list: CmdOut,
    pub profile_list: CmdOut,
    pub forward_list: CmdOut,
    pub service_list: CmdOut,
    pub settings: CmdOut,
    /// Sandbox whose detail commands were fetched, if any.
    pub target: Option<String>,
    pub sandbox_get: CmdOut,
    pub policy: CmdOut,
    pub policy_history: CmdOut,
    pub sandbox_providers: CmdOut,
    pub logs: CmdOut,
    pub ssh_config: CmdOut,
}

impl Snapshot {
    pub fn gateways(&self) -> Vec<Gateway> {
        gateways(&self.gateway_list)
    }
    pub fn sandboxes(&self) -> Vec<Sandbox> {
        sandboxes(&self.sandbox_list)
    }
    pub fn providers(&self) -> Vec<Object> {
        objects(&self.provider_list, "providers")
    }
    pub fn profiles(&self) -> Vec<Object> {
        objects(&self.profile_list, "profiles")
    }
    pub fn forwards(&self) -> Vec<Object> {
        objects(&self.forward_list, "forwards")
    }
    pub fn services(&self) -> Vec<Object> {
        objects(&self.service_list, "services")
    }
    pub fn templates(&self) -> Vec<Object> {
        objects(&self.template_list, "templates")
    }
    pub fn attached_providers(&self) -> Vec<Object> {
        objects(&self.sandbox_providers, "providers")
    }
    pub fn policy_revisions(&self) -> Vec<Object> {
        objects(&self.policy_history, "revisions")
    }
    /// Keys of `settings get --global`, which prints `key = value` lines.
    pub fn settings_rows(&self) -> Vec<(String, String)> {
        settings_rows(&self.settings)
    }
    pub fn health(&self) -> Health {
        let sandboxes = self.sandboxes();
        let running = sandboxes
            .iter()
            .filter(|sbx| matches!(sbx.phase.as_str(), "Ready" | "Running" | "ready"))
            .count();
        let attention = sandboxes
            .iter()
            .filter(|sbx| !matches!(sbx.phase.as_str(), "Ready" | "Running" | "ready"))
            .count();
        Health {
            connected: self.status.ok && self.status.text.contains("\"connected\""),
            sandboxes: sandboxes.len(),
            running,
            attention,
            providers: self.providers().len(),
            forwards: self.forwards().len(),
            services: self.services().len(),
            failures: [
                &self.status,
                &self.whoami,
                &self.gateway_list,
                &self.sandbox_list,
                &self.provider_list,
                &self.forward_list,
                &self.service_list,
            ]
            .iter()
            .filter(|out| !out.ok)
            .count(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Health {
    pub connected: bool,
    pub sandboxes: usize,
    pub running: usize,
    pub attention: usize,
    pub providers: usize,
    pub forwards: usize,
    pub services: usize,
    pub failures: usize,
}

/// One gateway registration.
#[derive(Clone, Debug, Default)]
pub struct Gateway {
    pub name: String,
    pub endpoint: String,
    pub auth: String,
    pub kind: String,
    pub source: String,
    pub active: bool,
    pub is_remote: bool,
}

/// One sandbox, reduced to what the panel renders.
#[derive(Clone, Debug, Default)]
pub struct Sandbox {
    pub name: String,
    pub phase: String,
    pub workspace: String,
    pub created_at: String,
    pub policy_version: u64,
    pub id: String,
    /// `(label, detail)` from `conditions`, already filtered to non-Ready ones.
    pub conditions: Vec<(String, String)>,
    pub annotations: Vec<(String, String)>,
}

/// A generic JSON object flattened to `(key, value)` pairs, sorted by key.
/// Used where the upstream schema is not fixed (providers, forwards, ...).
#[derive(Clone, Debug, Default)]
pub struct Object {
    pub rows: Vec<(String, String)>,
}

impl Object {
    pub fn get(&self, key: &str) -> &str {
        self.rows
            .iter()
            .find(|(name, _)| name == key)
            .map_or("", |(_, value)| value.as_str())
    }
}

// ============================================================================
// Parsing
// ============================================================================

fn text(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::String(raw)) => raw.clone(),
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::Bool(flag)) => flag.to_string(),
        _ => String::new(),
    }
}

fn array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map_or(&[], |items| items.as_slice())
}

fn parse(raw: &CmdOut) -> Option<Value> {
    serde_json::from_str::<Value>(raw.text.trim()).ok()
}

/// One-line rendering of a JSON scalar, for the panel's key/value rows.
pub fn json_scalar(value: &Value) -> String {
    match value {
        Value::String(raw) => raw.clone(),
        Value::Null => "—".to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        other => other.to_string(),
    }
}

pub fn gateways(raw: &CmdOut) -> Vec<Gateway> {
    let Some(root) = parse(raw) else {
        return Vec::new();
    };
    let items = if root.is_array() {
        root.as_array().map_or(&[][..], |items| items.as_slice())
    } else {
        array(&root, "gateways")
    };
    items
        .iter()
        .map(|item| Gateway {
            name: text(item, "name"),
            endpoint: text(item, "endpoint"),
            auth: text(item, "auth"),
            kind: text(item, "type"),
            source: text(item, "source"),
            active: item.get("active").and_then(Value::as_bool).unwrap_or(false),
            is_remote: item
                .get("is_remote")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
        .collect()
}

fn parse_sandbox(item: &Value) -> Sandbox {
    let conditions = array(item, "conditions")
        .iter()
        .filter(|condition| {
            text(condition, "type") != "Ready" || text(condition, "status") != "True"
        })
        .map(|condition| {
            (
                format!(
                    "{} · {}",
                    text(condition, "type"),
                    text(condition, "reason")
                ),
                text(condition, "message"),
            )
        })
        .collect();
    Sandbox {
        name: text(item, "name"),
        phase: text(item, "phase"),
        workspace: text(item, "workspace"),
        created_at: text(item, "created_at"),
        policy_version: item
            .get("current_policy_version")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        id: text(item, "id"),
        conditions,
        annotations: item
            .get("annotations")
            .and_then(Value::as_object)
            .map(|map| {
                let mut pairs: Vec<(String, String)> = map
                    .iter()
                    .map(|(key, value)| (key.clone(), json_scalar(value)))
                    .collect();
                pairs.sort_by(|a, b| a.0.cmp(&b.0));
                pairs
            })
            .unwrap_or_default(),
    }
}

pub fn sandboxes(raw: &CmdOut) -> Vec<Sandbox> {
    let Some(root) = parse(raw) else {
        return Vec::new();
    };
    array(&root, "sandboxes")
        .iter()
        .map(parse_sandbox)
        .collect()
}

/// `sandbox get -o json` returns one object, not a `sandboxes` envelope.
pub fn sandbox_one(raw: &CmdOut) -> Option<Sandbox> {
    let root = parse(raw)?;
    if let Some(items) = root.get("sandboxes").and_then(Value::as_array) {
        return items.first().map(parse_sandbox);
    }
    if root.is_object() {
        return Some(parse_sandbox(&root));
    }
    None
}

/// Flatten `root.<key>[]` (or a bare array) into key/value rows.
pub fn objects(raw: &CmdOut, key: &str) -> Vec<Object> {
    let Some(root) = parse(raw) else {
        return Vec::new();
    };
    let items = if root.is_array() {
        root.as_array().map_or(&[][..], |items| items.as_slice())
    } else {
        array(&root, key)
    };
    items
        .iter()
        .map(|item| Object {
            rows: item
                .as_object()
                .map(|map| {
                    let mut rows: Vec<(String, String)> = map
                        .iter()
                        .map(|(name, value)| (name.clone(), compact(value)))
                        .collect();
                    rows.sort_by(|a, b| a.0.cmp(&b.0));
                    rows
                })
                .unwrap_or_default(),
        })
        .collect()
}

/// One-line rendering of a JSON value, for table cells.
fn compact(value: &Value) -> String {
    match value {
        Value::Array(items) => {
            let joined: Vec<String> = items.iter().take(4).map(json_scalar).collect();
            if items.len() > 4 {
                format!("{}, … (+{})", joined.join(", "), items.len() - 4)
            } else {
                joined.join(", ")
            }
        }
        Value::Object(map) => {
            let joined: Vec<String> = map
                .iter()
                .take(3)
                .map(|(key, value)| format!("{key}={}", json_scalar(value)))
                .collect();
            if map.len() > 3 {
                format!("{}, …", joined.join(" "))
            } else {
                joined.join(" ")
            }
        }
        other => json_scalar(other),
    }
}

/// `settings get` has no `-o json`; its output is `key = value` lines.
pub fn settings_rows(raw: &CmdOut) -> Vec<(String, String)> {
    raw.text
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let (key, value) = line.split_once('=')?;
            let key = key.trim();
            if key.is_empty() || key.contains(':') {
                return None;
            }
            Some((key.to_string(), value.trim().to_string()))
        })
        .collect()
}

/// `status -o json` summary: `(gateway, endpoint, auth, version)`.
pub fn status_summary(raw: &CmdOut) -> Vec<(String, String)> {
    let Some(root) = parse(raw) else {
        return vec![("status".into(), raw.headline())];
    };
    let auth = root
        .get("authentication")
        .map(|auth| format!("{} · {}", text(auth, "provider"), text(auth, "status")))
        .unwrap_or_default();
    vec![
        ("gateway".into(), text(&root, "gateway")),
        ("server".into(), text(&root, "server")),
        ("status".into(), text(&root, "status")),
        ("version".into(), text(&root, "version")),
        ("auth".into(), auth),
    ]
}

/// `whoami -o json` summary.
pub fn whoami_summary(raw: &CmdOut) -> Vec<(String, String)> {
    let Some(root) = parse(raw) else {
        return vec![("identity".into(), raw.headline())];
    };
    let roles = compact(root.get("roles").unwrap_or(&Value::Null));
    let scopes = compact(root.get("scopes").unwrap_or(&Value::Null));
    vec![
        ("subject".into(), text(&root, "subject")),
        ("display".into(), text(&root, "display_name")),
        ("provider".into(), text(&root, "identity_provider")),
        (
            "roles".into(),
            if roles.is_empty() {
                "—".into()
            } else {
                roles
            },
        ),
        (
            "scopes".into(),
            if scopes.is_empty() {
                "—".into()
            } else {
                scopes
            },
        ),
    ]
}

// ============================================================================
// Actions
// ============================================================================

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Refresh,
    Start(String),
    Stop(String),
    Suspend(String),
    Resume(String),
    Delete(String),
    Exec {
        sandbox: String,
        command: String,
    },
    ForwardStart {
        port: u16,
        sandbox: String,
    },
    ForwardStop {
        port: u16,
    },
    Expose {
        sandbox: String,
        port: u16,
        service: String,
    },
    Create {
        name: String,
    },
    PolicyDryRun {
        sandbox: String,
        endpoint: String,
        binary: String,
    },
    Attach {
        sandbox: String,
        provider: String,
    },
    Detach {
        sandbox: String,
        provider: String,
    },
    ProviderRefreshStatus,
}

impl Action {
    pub fn argv(&self) -> Vec<String> {
        let owned = |items: &[&str]| items.iter().map(|item| item.to_string()).collect();
        match self {
            Action::Refresh => owned(&["status", "-o", "json"]),
            Action::Start(name) => owned(&["sandbox", "start", name]),
            Action::Stop(name) => owned(&["sandbox", "stop", name]),
            Action::Suspend(name) => owned(&["sandbox", "suspend", name]),
            Action::Resume(name) => owned(&["sandbox", "resume", name]),
            Action::Delete(name) => owned(&["sandbox", "delete", name]),
            Action::Exec { sandbox, command } => {
                let mut args = owned(&["sandbox", "exec", sandbox, "--", "sh", "-c"]);
                args.push(command.clone());
                args
            }
            Action::ForwardStart { port, sandbox } => {
                owned(&["forward", "start", &port.to_string(), sandbox])
            }
            Action::ForwardStop { port } => owned(&["forward", "stop", &port.to_string()]),
            Action::Expose {
                sandbox,
                port,
                service,
            } => owned(&["service", "expose", sandbox, &port.to_string(), service]),
            Action::Create { name } => owned(&[
                "sandbox", "create", "--name", name, "--detach", "--", "sleep", "infinity",
            ]),
            Action::PolicyDryRun {
                sandbox,
                endpoint,
                binary,
            } => owned(&[
                "policy",
                "update",
                sandbox,
                "--add-endpoint",
                endpoint,
                "--rule-name",
                "panel-preview",
                "--binary",
                binary,
                "--dry-run",
            ]),
            Action::Attach { sandbox, provider } => {
                owned(&["sandbox", "provider", "attach", sandbox, provider])
            }
            Action::Detach { sandbox, provider } => {
                owned(&["sandbox", "provider", "detach", sandbox, provider])
            }
            Action::ProviderRefreshStatus => owned(&["provider", "refresh", "status"]),
        }
    }

    /// Mutating actions need a second click; read-only ones run immediately.
    pub fn is_mutating(&self) -> bool {
        matches!(
            self,
            Action::Start(_)
                | Action::Stop(_)
                | Action::Suspend(_)
                | Action::Resume(_)
                | Action::Delete(_)
                | Action::Exec { .. }
                | Action::ForwardStart { .. }
                | Action::ForwardStop { .. }
                | Action::Expose { .. }
                | Action::Create { .. }
                | Action::PolicyDryRun { .. }
                | Action::Attach { .. }
                | Action::Detach { .. }
        )
    }

    /// Destructive enough to sit behind an explicit warning.
    pub fn is_destructive(&self) -> bool {
        matches!(
            self,
            Action::Delete(_) | Action::Detach { .. } | Action::ForwardStop { .. }
        )
    }

    /// Read-only probe that should be verified on the UI thread.
    pub fn is_probe(&self) -> bool {
        matches!(self, Action::Refresh | Action::ProviderRefreshStatus)
    }

    pub fn label(&self) -> &'static str {
        match self {
            Action::Refresh => "刷新",
            Action::Start(_) => "START",
            Action::Stop(_) => "STOP",
            Action::Suspend(_) => "SUSPEND",
            Action::Resume(_) => "RESUME",
            Action::Delete(_) => "DELETE",
            Action::Exec { .. } => "RUN",
            Action::ForwardStart { .. } => "START",
            Action::ForwardStop { .. } => "STOP",
            Action::Expose { .. } => "EXPOSE",
            Action::Create { .. } => "CREATE",
            Action::PolicyDryRun { .. } => "DRY RUN",
            Action::Attach { .. } => "ATTACH",
            Action::Detach { .. } => "DETACH",
            Action::ProviderRefreshStatus => "REFRESH STATUS",
        }
    }

    pub fn command(&self) -> String {
        format!("{BIN} {}", self.argv().join(" "))
    }
}

#[derive(Clone, Debug)]
pub struct Outcome {
    pub command: String,
    pub ok: bool,
    pub output: String,
}

pub fn run(action: &Action) -> Outcome {
    let argv = action.argv();
    let borrowed: Vec<&str> = argv.iter().map(String::as_str).collect();
    let secs = match action {
        Action::Create { .. } => 180,
        Action::Exec { .. } => 120,
        _ => 30,
    };
    let out = cli(&borrowed, secs);
    Outcome {
        command: action.command(),
        ok: out.ok,
        output: if out.is_empty() {
            format!("{} 已执行（无输出）", action.command())
        } else {
            out.text
        },
    }
}

// ============================================================================
// Probe
// ============================================================================

/// Read the whole gateway. `target` selects the sandbox whose detail views
/// (policy / logs / ssh / providers) are fetched too.
pub fn probe(target: Option<&str>) -> Snapshot {
    let status = cli_json(&["status"], 20);
    let whoami = cli_json(&["whoami"], 20);
    let gateway_list = cli_json(&["gateway", "list"], 20);
    let gateway_info = cli(&["gateway", "info"], 25);
    let doctor = cli(&["doctor", "check"], 40);
    let sandbox_list = cli_json(&["sandbox", "list"], 25);
    let template_list = cli_json(&["sandbox", "template", "list"], 20);
    let provider_list = cli_json(&["provider", "list"], 20);
    let profile_list = cli_json(&["profile", "list"], 20);
    let forward_list = cli_json(&["forward", "list"], 20);
    let service_list = cli_json(&["service", "list"], 20);
    let settings = cli(&["settings", "get", "--global"], 20);

    let resolved = target
        .map(str::to_string)
        .or_else(|| sandboxes(&sandbox_list).first().map(|sbx| sbx.name.clone()));

    let mut snap = Snapshot {
        probed_at: crate::sys::local_clock(),
        status,
        whoami,
        gateway_list,
        gateway_info,
        doctor,
        sandbox_list,
        template_list,
        provider_list,
        profile_list,
        forward_list,
        service_list,
        settings,
        target: resolved.clone(),
        ..Snapshot::default()
    };

    if let Some(name) = resolved {
        snap.sandbox_get = cli_json(&["sandbox", "get", &name], 20);
        snap.policy = cli_json(&["policy", "get", &name], 20);
        snap.policy_history = cli_json(&["policy", "list", &name], 20);
        snap.sandbox_providers = cli_json(&["sandbox", "provider", "list", &name], 20);
        snap.logs = cli(&["logs", &name, "-n", "60"], 25);
        snap.ssh_config = cli(&["sandbox", "ssh-config", &name], 20);
    }
    snap
}

/// UI state for the extension: last snapshot, in-flight action, and a bounded
/// action log that feeds the timeline and the audit view.
pub struct State {
    pub snap: Snapshot,
    pub busy: bool,
    pub target: Option<String>,
    pub armed: Option<Action>,
    pub log: Vec<Outcome>,
    /// Last `sandbox exec` output, kept separate so the console does not get
    /// overwritten by lifecycle commands.
    pub exec_output: Option<Outcome>,
    pub checked: bool,
}

impl State {
    pub fn new() -> Self {
        Self {
            snap: Snapshot::default(),
            busy: false,
            target: None,
            armed: None,
            log: Vec::new(),
            exec_output: None,
            checked: false,
        }
    }

    pub fn absorb(&mut self, snapshot: Snapshot) {
        if self.target.is_none() {
            self.target = snapshot.target.clone();
        }
        self.snap = snapshot;
        self.checked = true;
    }

    pub fn record(&mut self, outcome: Outcome) {
        if outcome.command.contains("sandbox exec") {
            self.exec_output = Some(outcome.clone());
        }
        self.log.push(outcome);
        if self.log.len() > 40 {
            self.log.remove(0);
        }
    }

    pub fn sandbox(&self, name: &str) -> Option<Sandbox> {
        self.snap
            .sandboxes()
            .into_iter()
            .find(|sbx| sbx.name == name)
    }

    /// Selected sandbox, falling back to the first one the gateway reports.
    pub fn active_sandbox(&self) -> Option<String> {
        self.target
            .clone()
            .or_else(|| self.snap.sandboxes().first().map(|sbx| sbx.name.clone()))
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandboxes_parse_real_payload() {
        let raw = CmdOut {
            ok: true,
            text: r#"{"next_page_token":"","sandboxes":[
                {"name":"os-demo","phase":"Ready","workspace":"default",
                 "created_at":"2026-11-13 06:47:47","current_policy_version":1,
                 "id":"c710b436","conditions":[
                    {"type":"Ready","status":"True","reason":"DependenciesReady","message":"ok"},
                    {"type":"WorkloadIdentity","status":"True","reason":"DriverResolved","message":"uid 1000"}],
                 "annotations":{"internal.openshell.ai/isolation-backend":"openshell-sandbox"}}
            ]}"#
                .into(),
        };
        let parsed = sandboxes(&raw);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "os-demo");
        assert_eq!(parsed[0].phase, "Ready");
        assert_eq!(parsed[0].policy_version, 1);
        // Ready/True conditions are noise; the rest stay visible.
        assert_eq!(parsed[0].conditions.len(), 1);
        assert!(parsed[0].annotations[0].0.contains("isolation-backend"));
    }

    #[test]
    fn single_sandbox_object_parses_too() {
        let raw = CmdOut {
            ok: true,
            text: r#"{"name":"os-demo","phase":"Ready","workspace":"default",
                 "created_at":"2026-11-13 06:47:47","current_policy_version":3,
                 "id":"c710b436","conditions":[],"annotations":{}}"#
                .into(),
        };
        let one = sandbox_one(&raw).expect("single object");
        assert_eq!(one.name, "os-demo");
        assert_eq!(one.policy_version, 3);
        assert!(sandbox_one(&CmdOut::failure("boom")).is_none());
    }

    #[test]
    fn broken_output_yields_nothing_instead_of_fake_state() {
        let raw = CmdOut {
            ok: false,
            text: "Error: No sandbox name provided".into(),
        };
        assert!(sandboxes(&raw).is_empty());
        assert!(gateways(&raw).is_empty());
        assert!(objects(&raw, "providers").is_empty());
    }

    #[test]
    fn settings_lines_parse_without_the_header() {
        let raw = CmdOut {
            ok: true,
            text: "Scope:         global\nSettings Rev:  0\nSettings:\n  ocsf_json_enabled = <unset>\n  proposal_approval_mode = manual"
                .into(),
        };
        let rows = settings_rows(&raw);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, "ocsf_json_enabled");
        assert_eq!(rows[1].1, "manual");
    }

    #[test]
    fn mutating_and_destructive_flags_are_explicit() {
        assert!(!Action::Refresh.is_mutating());
        assert!(Action::Stop("a".into()).is_mutating());
        assert!(Action::Delete("a".into()).is_destructive());
        // dry-run never reaches the gateway
        assert!(
            Action::PolicyDryRun {
                sandbox: "a".into(),
                endpoint: "api.github.com:443:read-only:rest:enforce".into(),
                binary: "/usr/bin/gh".into(),
            }
            .argv()
            .iter()
            .any(|arg| arg == "--dry-run")
        );
        // exec goes through `sh -c` after an explicit `--`
        let argv = Action::Exec {
            sandbox: "a".into(),
            command: "uname -a".into(),
        }
        .argv();
        assert_eq!(&argv[..5], ["sandbox", "exec", "a", "--", "sh"]);
        assert_eq!(argv[5], "-c");
        assert_eq!(argv[6], "uname -a");
    }
}
