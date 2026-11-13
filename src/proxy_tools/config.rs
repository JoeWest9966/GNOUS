//! Persisted configuration for the proxy-tools extension.
//!
//! Kept intentionally small and dependency-free in shape: a JSON file under
//! `$XDG_CONFIG_HOME/gnous/proxy-tools.json` (or `~/.config/…`). The defaults
//! reproduce today's working setup, so installing GNOUS changes nothing until
//! the operator asks for it.

use std::net::{SocketAddr, ToSocketAddrs};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Where the local relay listens today (`jproxy` defaults).
pub const DEFAULT_LISTEN: &str = "127.0.0.1:31001"; // safety-allow: 复刻现状默认值，绑定前仍过 guard
/// j1900's sing-box mixed inbound.
pub const DEFAULT_UPSTREAM: &str = "192.168.1.7:31001"; // safety-allow: 复刻现状默认值（上游只读连接）
pub const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 300;
pub const DEFAULT_RETRIES: u32 = 2;
pub const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 8;

/// How `no_proxy` is generated from the machine's real interfaces.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum NoProxyMode {
    /// Enumerate every host of each global /24 (what Node/Bun actually need).
    Exact24,
    /// Only localhost plus the gateway/host IPs, no /24 expansion.
    Minimal,
    /// Leave `no_proxy` alone and report what is there.
    Manual,
}

impl NoProxyMode {
    pub fn label(self) -> &'static str {
        match self {
            NoProxyMode::Exact24 => "整段精确 IP",
            NoProxyMode::Minimal => "仅本机 + 网关",
            NoProxyMode::Manual => "不生成（手工）",
        }
    }

    pub fn next(self) -> Self {
        match self {
            NoProxyMode::Exact24 => NoProxyMode::Minimal,
            NoProxyMode::Minimal => NoProxyMode::Manual,
            NoProxyMode::Manual => NoProxyMode::Exact24,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct Config {
    pub listen: String,
    pub upstream: String,
    pub idle_timeout_secs: u64,
    pub connect_timeout_secs: u64,
    pub retries: u32,
    pub no_proxy_mode: NoProxyMode,
    /// Set when the operator handover to the GNOUS relay has been recorded.
    pub adopted: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: DEFAULT_LISTEN.into(),
            upstream: DEFAULT_UPSTREAM.into(),
            idle_timeout_secs: DEFAULT_IDLE_TIMEOUT_SECS,
            connect_timeout_secs: DEFAULT_CONNECT_TIMEOUT_SECS,
            retries: DEFAULT_RETRIES,
            no_proxy_mode: NoProxyMode::Exact24,
            adopted: false,
        }
    }
}

impl Config {
    pub fn listen_addr(&self) -> Result<SocketAddr, String> {
        parse_addr(&self.listen)
    }

    pub fn upstream_addr(&self) -> Result<SocketAddr, String> {
        parse_addr(&self.upstream)
    }

    /// Problems that would make the installed unit useless, in plain language.
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if let Err(error) = self.listen_addr() {
            problems.push(format!("监听地址无效：{error}"));
        }
        if let Err(error) = self.upstream_addr() {
            problems.push(format!("上游地址无效：{error}"));
        }
        if self.idle_timeout_secs < 30 {
            problems.push("空闲超时过短（< 30s），会把正常长连接切掉".into());
        }
        if self.connect_timeout_secs == 0 || self.connect_timeout_secs > 60 {
            problems.push("连接超时应在 1–60 秒之间".into());
        }
        if self.retries > 5 {
            problems.push("重试次数过多（> 5），失败时会长时间挂住".into());
        }
        problems
    }
}

pub fn parse_addr(text: &str) -> Result<SocketAddr, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("地址为空".into());
    }
    trimmed
        .to_socket_addrs()
        .map_err(|error| format!("无法解析 {trimmed}: {error}"))?
        .next()
        .ok_or_else(|| format!("{trimmed} 没有解析出地址"))
}

pub fn config_dir() -> PathBuf {
    let base = std::env::var_os("GNOUS_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .unwrap_or_else(|| home().join(".config"));
    base.join("gnous")
}

pub fn config_file() -> PathBuf {
    config_dir().join("proxy-tools.json")
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/root"), PathBuf::from)
}

pub fn load() -> Config {
    match std::fs::read_to_string(config_file()) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|error| {
            eprintln!(
                "gnous: {} 解析失败（{error}），回退默认配置",
                config_file().display()
            );
            Config::default()
        }),
        Err(_) => Config::default(),
    }
}

/// Atomic write: temp file in the same directory, then rename.
pub fn save(config: &Config) -> Result<PathBuf, String> {
    let path = config_file();
    let dir = config_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("创建 {} 失败: {error}", dir.display()))?;
    let text = serde_json::to_string_pretty(config).map_err(|e| format!("序列化配置失败: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, format!("{text}\n"))
        .map_err(|e| format!("写 {} 失败: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("落盘 {} 失败: {e}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_todays_working_setup() {
        let config = Config::default();
        assert_eq!(config.listen, DEFAULT_LISTEN);
        assert_eq!(config.upstream, DEFAULT_UPSTREAM);
        assert_eq!(config.idle_timeout_secs, 300);
        assert_eq!(config.retries, 2);
        assert!(!config.adopted);
        assert!(config.validate().is_empty(), "{:?}", config.validate());
    }

    #[test]
    fn address_parsing_rejects_garbage() {
        assert!(parse_addr(DEFAULT_LISTEN).is_ok());
        assert!(parse_addr("[::1]:1").is_ok());
        assert!(parse_addr("").is_err());
        assert!(parse_addr("127.0.0.1").is_err());
    }

    #[test]
    fn validation_catches_risky_numbers() {
        let config = Config {
            idle_timeout_secs: 5,
            retries: 9,
            connect_timeout_secs: 300,
            ..Config::default()
        };
        let problems = config.validate();
        assert!(problems.iter().any(|p| p.contains("空闲超时过短")));
        assert!(problems.iter().any(|p| p.contains("重试次数过多")));
        assert!(problems.iter().any(|p| p.contains("连接超时")));
    }

    #[test]
    fn round_trips_through_json() {
        let config = Config {
            retries: 4,
            no_proxy_mode: NoProxyMode::Minimal,
            ..Config::default()
        };
        let text = serde_json::to_string(&config).unwrap();
        let back: Config = serde_json::from_str(&text).unwrap();
        assert_eq!(config, back);
    }

    #[test]
    fn partial_json_keeps_defaults() {
        let back: Config = serde_json::from_str(r#"{"retries": 3}"#).unwrap();
        assert_eq!(back.retries, 3);
        assert_eq!(back.listen, DEFAULT_LISTEN);
        assert_eq!(back.no_proxy_mode, NoProxyMode::Exact24);
    }
}
