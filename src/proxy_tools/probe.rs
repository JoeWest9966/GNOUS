//! Read-only probing.
//!
//! Nothing here writes, restarts, or connects to the internet: listeners come
//! from `/proc/net/tcp*`, unit state from `systemctl --user show` (read verb
//! only), control-plane state from files and `systemctl --user show-environment`.
//! The single outbound connect is a TCP handshake against the *upstream* on the
//! LAN, which is what `jproxy status` already did.

use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::exec;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UnitState {
    pub load: String,
    pub active: String,
    pub sub: String,
    pub enabled: String,
    pub main_pid: Option<u32>,
    pub exec_start: String,
    pub restarts: Option<u32>,
    pub present: bool,
}

impl UnitState {
    pub fn running(&self) -> bool {
        self.active == "active"
    }
}

pub fn parse_properties(text: &str) -> UnitState {
    let mut state = UnitState::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().to_string();
        match key.trim() {
            "LoadState" => {
                state.load = value.clone();
                state.present = value != "not-found";
            }
            "ActiveState" => state.active = value,
            "SubState" => state.sub = value,
            "UnitFileState" => state.enabled = value,
            "ExecMainPID" => state.main_pid = value.parse().ok().filter(|pid| *pid > 0),
            "ExecStart" => state.exec_start = value,
            "NRestarts" => state.restarts = value.parse().ok(),
            _ => {}
        }
    }
    state
}

pub fn unit_state(unit: &str) -> Result<UnitState, String> {
    exec::unit_properties(unit).map(|text| parse_properties(&text))
}

/// True when something is listening on `port` locally, read from procfs so that
/// probing does not create a connection to the live relay.
pub fn listening_on(port: u16) -> bool {
    for table in ["/proc/net/tcp", "/proc/net/tcp6"] {
        let Ok(text) = std::fs::read_to_string(table) else {
            continue;
        };
        if table_listens_on(&text, port) {
            return true;
        }
    }
    false
}

/// Pure parser, so the rule is testable without a listener.
pub fn table_listens_on(text: &str, port: u16) -> bool {
    for line in text.lines().skip(1) {
        let mut fields = line.split_whitespace();
        let Some(_slot) = fields.next() else { continue };
        let Some(local) = fields.next() else { continue };
        let Some(_remote) = fields.next() else {
            continue;
        };
        let Some(state) = fields.next() else { continue };
        if state != "0A" {
            continue; // 0A == TCP_LISTEN
        }
        let Some((_, port_hex)) = local.rsplit_once(':') else {
            continue;
        };
        if u16::from_str_radix(port_hex, 16).ok() == Some(port) {
            return true;
        }
    }
    false
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UpstreamProbe {
    pub reachable: bool,
    pub latency_ms: u64,
    pub detail: String,
}

pub fn probe_upstream(addr: SocketAddr, timeout: Duration) -> UpstreamProbe {
    let started = Instant::now();
    match TcpStream::connect_timeout(&addr, timeout) {
        Ok(_) => UpstreamProbe {
            reachable: true,
            latency_ms: started.elapsed().as_millis() as u64,
            detail: "TCP 握手成功".into(),
        },
        Err(error) => UpstreamProbe {
            reachable: false,
            latency_ms: started.elapsed().as_millis() as u64,
            detail: error.to_string(),
        },
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ControlPlane {
    pub env_files: Vec<PathBuf>,
    pub env_keys: Vec<(String, String)>,
    pub user_manager_keys: Vec<(String, String)>,
    pub no_proxy: Option<String>,
    pub http_proxy: Option<String>,
}

pub fn control_plane() -> ControlPlane {
    let env_files = environment_d_files();
    let mut env_keys: Vec<(String, String)> = Vec::new();
    for file in &env_files {
        if let Ok(text) = std::fs::read_to_string(file) {
            env_keys.extend(parse_env_file(&text));
        }
    }
    let user_manager_keys = exec::run(&["systemctl", "--user", "show-environment"])
        .map(|text| parse_env_file(&text))
        .unwrap_or_default();
    let no_proxy = lookup(&env_keys, "no_proxy")
        .or_else(|| lookup(&user_manager_keys, "no_proxy"))
        .or_else(|| std::env::var("no_proxy").ok());
    let http_proxy = lookup(&env_keys, "http_proxy")
        .or_else(|| lookup(&user_manager_keys, "http_proxy"))
        .or_else(|| std::env::var("http_proxy").ok());
    ControlPlane {
        env_files,
        env_keys,
        user_manager_keys,
        no_proxy,
        http_proxy,
    }
}

fn lookup(pairs: &[(String, String)], key: &str) -> Option<String> {
    pairs
        .iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(key))
        .map(|(_, value)| value.clone())
}

/// Every `*.conf` drop-in under `~/.config/environment.d` that
/// actually defines proxy variables. We deliberately match on content instead of
/// hard-coding the legacy filename.
pub fn environment_d_files() -> Vec<PathBuf> {
    let dir = environment_d_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "conf"))
        .filter(|path| {
            std::fs::read_to_string(path)
                .map(|text| text.contains("_proxy=") || text.contains("_PROXY="))
                .unwrap_or(false)
        })
        .collect();
    files.sort();
    files
}

pub fn environment_d_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"));
    base.join("environment.d")
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/root"), PathBuf::from)
}

pub fn parse_env_file(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        if let Some((key, value)) = line.split_once('=') {
            out.push((key.trim().to_string(), value.trim().to_string()));
        }
    }
    out
}

/// Count ERROR-ish lines in the last `lines` lines of a log file.
pub fn log_error_tail(log: &Path, lines: usize) -> (usize, usize, Vec<String>) {
    let Ok(text) = std::fs::read_to_string(log) else {
        return (0, 0, Vec::new());
    };
    let all: Vec<&str> = text.lines().rev().take(lines).collect();
    let total = all.len();
    let mut samples = Vec::new();
    for line in &all {
        if line.contains("ERROR") || line.contains("Traceback") || line.contains("失败") {
            samples.push((*line).chars().take(160).collect());
        }
    }
    (total, samples.len(), samples)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROPERTIES: &str = "\
ActiveState=active
SubState=running
UnitFileState=enabled
ExecMainPID=440668
ExecStart={ path=/usr/bin/python3 ; argv[]=/usr/bin/python3 /home/root/GNOME/x.py serve }
NRestarts=0
";

    #[test]
    fn parses_systemctl_show_output() {
        let state = parse_properties(PROPERTIES);
        assert!(!state.present, "fixture has no LoadState");
        assert!(state.running());
        assert_eq!(state.main_pid, Some(440668));
        assert_eq!(state.enabled, "enabled");
        assert!(state.exec_start.contains("python3"));
        assert_eq!(state.restarts, Some(0));
    }

    #[test]
    fn missing_unit_is_not_running_and_not_present() {
        let state = parse_properties("LoadState=not-found\nActiveState=inactive\nUnitFileState=\n");
        assert!(!state.running());
        assert!(!state.present);
        assert_eq!(state.main_pid, None);
    }

    #[test]
    fn loaded_unit_is_present_even_when_inactive() {
        let state =
            parse_properties("LoadState=loaded\nActiveState=inactive\nUnitFileState=disabled\n");
        assert!(state.present);
        assert!(!state.running());
    }

    #[test]
    fn detect_listener_from_proc_table() {
        // 0x1F90 = 8080 listening on 0.0.0.0; 0x1F91 is an established socket.
        let table = "\
  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 00000000:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 1
   1: 0100007F:1F91 0100007F:9C40 01 00000000:00000000 00:00000000 00000000     0        0 2
";
        assert!(table_listens_on(table, 8080));
        assert!(!table_listens_on(table, 8081), "established is not listen");
        assert!(!table_listens_on(table, 9999));
    }

    #[test]
    fn env_files_parse_export_and_plain_forms() {
        let parsed = parse_env_file("export http_proxy=http://127.0.0.1:1\nno_proxy=a,b\n# c=d\n");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].0, "http_proxy");
        assert_eq!(parsed[1].1, "a,b");
    }

    #[test]
    fn log_tail_counts_errors() {
        let dir = std::env::temp_dir().join(format!("gnous-proxy-log-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("relay.log");
        std::fs::write(
            &path,
            "INFO ok\nERROR upstream refused\nINFO ok\nTraceback boom\n",
        )
        .unwrap();
        let (total, errors, samples) = log_error_tail(&path, 100);
        assert_eq!(total, 4);
        assert_eq!(errors, 2);
        assert!(samples[0].contains("Traceback") || samples[0].contains("ERROR"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_log_is_not_an_error() {
        let (total, errors, samples) = log_error_tail(Path::new("/nonexistent/relay.log"), 10);
        assert_eq!((total, errors, samples.len()), (0, 0, 0));
    }
}
