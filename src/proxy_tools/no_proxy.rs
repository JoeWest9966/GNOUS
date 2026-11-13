//! `no_proxy` / GNOME `ignore-hosts` generation from the machine's real
//! interfaces.
//!
//! Why this exists: Node / Bun only match `no_proxy` against exact host names or
//! domain suffixes — a CIDR such as `192.168.0.0/16` is silently ignored, so
//! anything driving a local API through Bun gets pushed into the proxy and dies.
//! The previous shell implementation hard-coded "enumerate all 254 hosts of the
//! current /24"; we keep that behaviour but derive it from the interfaces that
//! actually exist, and we can tell the operator exactly which hosts are missing.

use std::net::Ipv4Addr;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Interface {
    pub name: String,
    pub ip: Ipv4Addr,
    pub prefix_len: u8,
}

impl Interface {
    pub fn network(&self) -> String {
        let octets = self.ip.octets();
        format!("{}.{}.{}.{}", octets[0], octets[1], octets[2], octets[3])
    }

    /// Hosts of this /24, excluding network and broadcast addresses.
    pub fn exact_hosts(&self) -> Vec<String> {
        if self.prefix_len != 24 {
            return vec![self.network()];
        }
        let octets = self.ip.octets();
        (1..=254)
            .map(|last| format!("{}.{}.{}.{last}", octets[0], octets[1], octets[2]))
            .collect()
    }

    pub fn gateway_guess(&self) -> String {
        let octets = self.ip.octets();
        format!("{}.{}.{}.1", octets[0], octets[1], octets[2])
    }
}

/// Parse `ip -4 -o addr show scope global` output.
pub fn parse_interfaces(text: &str) -> Vec<Interface> {
    let mut out = Vec::new();
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        // `2: wlp1s0    inet 192.168.1.3/24 brd ...`
        let Some(_index) = fields.next() else {
            continue;
        };
        let Some(name) = fields.next() else { continue };
        let name = name.trim_end_matches(':').to_string();
        let Some(kind) = fields.next() else { continue };
        if kind != "inet" {
            continue;
        }
        let Some(cidr) = fields.next() else { continue };
        let Some((ip, prefix)) = cidr.split_once('/') else {
            continue;
        };
        let Ok(ip) = ip.parse::<Ipv4Addr>() else {
            continue;
        };
        let Ok(prefix_len) = prefix.parse::<u8>() else {
            continue;
        };
        out.push(Interface {
            name,
            ip,
            prefix_len,
        });
    }
    out
}

pub fn build_no_proxy(interfaces: &[Interface], mode: super::config::NoProxyMode) -> String {
    use super::config::NoProxyMode;
    let mut entries: Vec<String> = vec!["localhost".into(), "127.0.0.1".into(), "::1".into()];
    match mode {
        NoProxyMode::Manual => return String::new(),
        NoProxyMode::Minimal => {
            for interface in interfaces {
                entries.push(interface.network());
                entries.push(interface.gateway_guess());
            }
        }
        NoProxyMode::Exact24 => {
            for interface in interfaces {
                entries.extend(interface.exact_hosts());
            }
        }
    }
    entries.push(".local".into());
    entries.push(".lan".into());
    entries.push(".internal".into());
    dedup(entries).join(",")
}

/// GNOME's `ignore-hosts` is a GVariant string list; GIO understands CIDR.
pub fn build_ignore_hosts(interfaces: &[Interface]) -> String {
    let mut entries: Vec<String> = vec!["localhost".into(), "127.0.0.1".into(), "::1".into()];
    for interface in interfaces {
        let octets = interface.ip.octets();
        entries.push(format!("{}.{}.{}.0/24", octets[0], octets[1], octets[2]));
        entries.push(interface.gateway_guess());
        entries.push(interface.network());
    }
    entries.push("192.168.0.0/16".into());
    entries.push("10.0.0.0/8".into());
    entries.push("172.16.0.0/12".into());
    entries.push("*.local".into());
    let quoted: Vec<String> = dedup(entries)
        .into_iter()
        .map(|entry| format!("'{entry}'"))
        .collect();
    format!("[{}]", quoted.join(", "))
}

/// Which hosts of the machine's own subnets are **not** covered by the current
/// `no_proxy` value. Empty means "nothing missing".
pub fn missing_hosts(no_proxy: &str, interfaces: &[Interface]) -> Vec<String> {
    let entries: Vec<&str> = no_proxy.split(',').map(str::trim).collect();
    let mut missing = Vec::new();
    for interface in interfaces {
        for host in interface.exact_hosts() {
            if !entries.iter().any(|entry| *entry == host) && !missing.contains(&host) {
                missing.push(host);
            }
        }
    }
    missing
}

/// True when the current value covers at least the machine's own address.
pub fn covers_self(no_proxy: &str, interfaces: &[Interface]) -> bool {
    interfaces.iter().all(|interface| {
        no_proxy
            .split(',')
            .map(str::trim)
            .any(|entry| entry == interface.network())
    })
}

fn dedup(entries: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(entries.len());
    for entry in entries {
        if !out.iter().any(|existing| *existing == entry) {
            out.push(entry);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy_tools::config::NoProxyMode;

    const SAMPLE: &str = "\
2: wlp1s0    inet 192.168.1.3/24 brd 192.168.1.255 scope global dynamic wlp1s0
3: docker0    inet 172.17.0.1/16 brd 172.17.255.255 scope global docker0
";

    #[test]
    fn parses_real_ip_output() {
        let interfaces = parse_interfaces(SAMPLE);
        assert_eq!(interfaces.len(), 2);
        assert_eq!(interfaces[0].name, "wlp1s0");
        assert_eq!(interfaces[0].network(), "192.168.1.3");
        assert_eq!(interfaces[0].prefix_len, 24);
        assert_eq!(interfaces[1].name, "docker0");
        assert_eq!(interfaces[1].prefix_len, 16);
    }

    #[test]
    fn exact_mode_enumerates_the_whole_slash_24() {
        let interfaces = parse_interfaces(SAMPLE);
        let value = build_no_proxy(&interfaces, NoProxyMode::Exact24);
        let entries: Vec<&str> = value.split(',').collect();
        assert!(entries.contains(&"192.168.1.1"));
        assert!(entries.contains(&"192.168.1.254"));
        assert!(entries.contains(&"192.168.1.3"));
        // A /16 is not expanded to 65k entries; only its own address is listed.
        assert!(entries.contains(&"172.17.0.1"));
        assert!(!entries.contains(&"172.17.0.254"));
        assert!(entries.contains(&".lan"));
        assert_eq!(
            entries
                .iter()
                .filter(|entry| **entry == "192.168.1.7")
                .count(),
            1,
            "no duplicates"
        );
    }

    #[test]
    fn minimal_mode_keeps_only_the_local_address_and_gateway() {
        let interfaces = parse_interfaces(SAMPLE);
        let value = build_no_proxy(&interfaces, NoProxyMode::Minimal);
        assert!(value.contains("192.168.1.3"));
        assert!(value.contains("192.168.1.1"));
        assert!(!value.contains("192.168.1.42"));
    }

    #[test]
    fn manual_mode_returns_nothing_so_callers_keep_the_current_value() {
        let interfaces = parse_interfaces(SAMPLE);
        assert!(build_no_proxy(&interfaces, NoProxyMode::Manual).is_empty());
    }

    #[test]
    fn missing_hosts_reports_the_gap_in_a_minimal_value() {
        let interfaces = parse_interfaces(SAMPLE);
        let minimal = build_no_proxy(&interfaces, NoProxyMode::Minimal);
        let missing = missing_hosts(&minimal, &interfaces);
        assert!(missing.contains(&"192.168.1.7".to_string()));
        let exact = build_no_proxy(&interfaces, NoProxyMode::Exact24);
        assert!(missing_hosts(&exact, &interfaces).is_empty());
        assert!(covers_self(&minimal, &interfaces));
    }

    #[test]
    fn ignore_hosts_is_a_gvariant_string_list() {
        let interfaces = parse_interfaces(SAMPLE);
        let value = build_ignore_hosts(&interfaces);
        assert!(value.starts_with('[') && value.ends_with(']'));
        assert!(value.contains("'192.168.1.0/24'"));
        assert!(value.contains("'*.local'"));
    }

    #[test]
    fn empty_input_never_panics() {
        assert!(parse_interfaces("").is_empty());
        let value = build_no_proxy(&[], NoProxyMode::Exact24);
        assert_eq!(value, "localhost,127.0.0.1,::1,.local,.lan,.internal");
        assert!(missing_hosts(&value, &[]).is_empty());
    }
}
