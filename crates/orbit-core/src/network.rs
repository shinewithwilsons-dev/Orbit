//! Network view for Orbit Shield: which programs are connected where, and
//! a one-click firewall block (elevated with the OS's own password prompt).

use anyhow::Result;
use std::collections::HashMap;
use std::process::Command;

#[derive(Debug, Clone, PartialEq)]
pub struct Connection {
    pub proto: String,
    pub local: String,
    pub remote: String,
    pub state: String,
    pub pid: Option<u32>,
    pub process: String,
}

impl Connection {
    pub fn remote_ip(&self) -> Option<String> {
        let r = self.remote.trim();
        if r.is_empty() || r.starts_with('*') || r.starts_with("0.0.0.0") || r == "[::]:0" {
            return None;
        }
        // Split off the port: "1.2.3.4:443" or "[2001:db8::1]:443".
        let ip = if let Some(rest) = r.strip_prefix('[') {
            rest.split(']').next()?.to_string()
        } else {
            r.rsplit_once(':')
                .map(|(ip, _)| ip.to_string())
                .unwrap_or_else(|| r.to_string())
        };
        if ip == "127.0.0.1" || ip == "::1" || ip.is_empty() {
            None
        } else {
            Some(ip)
        }
    }
}

/// Active connections (established and listening).
pub fn connections() -> Result<Vec<Connection>> {
    let mut v = if cfg!(windows) {
        parse_netstat_windows(&run("netstat", &["-ano"])?)
    } else if cfg!(target_os = "macos") {
        parse_lsof(&run("lsof", &["-i", "-n", "-P"])?)
    } else if crate::exec::which("ss").is_some() {
        parse_ss(&run("ss", &["-tunapH"])?)
    } else {
        parse_lsof(&run("lsof", &["-i", "-n", "-P"])?)
    };
    // Fill in process names from the process table where missing.
    let mut sys = sysinfo::System::new();
    sys.refresh_processes();
    let names: HashMap<u32, String> = sys
        .processes()
        .iter()
        .map(|(p, pr)| (p.as_u32(), pr.name().to_string()))
        .collect();
    for c in v.iter_mut() {
        if c.process.is_empty() {
            if let Some(n) = c.pid.and_then(|p| names.get(&p)) {
                c.process = n.clone();
            }
        }
    }
    v.sort_by(|a, b| a.process.to_lowercase().cmp(&b.process.to_lowercase()));
    Ok(v)
}

fn run(cmd: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(cmd).args(args).output()?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn parse_netstat_windows(text: &str) -> Vec<Connection> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            match f.first().copied() {
                Some("TCP") if f.len() >= 5 => Some(Connection {
                    proto: "TCP".into(),
                    local: f[1].into(),
                    remote: f[2].into(),
                    state: f[3].into(),
                    pid: f[4].parse().ok(),
                    process: String::new(),
                }),
                Some("UDP") if f.len() >= 4 => Some(Connection {
                    proto: "UDP".into(),
                    local: f[1].into(),
                    remote: f[2].into(),
                    state: String::new(),
                    pid: f[3].parse().ok(),
                    process: String::new(),
                }),
                _ => None,
            }
        })
        .filter(|c| c.state != "TIME_WAIT")
        .collect()
}

/// `ss -tunapH`: Netid State Recv-Q Send-Q Local Peer Process
pub fn parse_ss(text: &str) -> Vec<Connection> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() < 6 {
                return None;
            }
            let procs = f.get(6..).map(|p| p.join(" ")).unwrap_or_default();
            let (process, pid) = parse_ss_users(&procs);
            Some(Connection {
                proto: f[0].to_uppercase(),
                local: f[4].into(),
                remote: f[5].into(),
                state: f[1].into(),
                pid,
                process,
            })
        })
        .filter(|c| c.state != "TIME-WAIT")
        .collect()
}

fn parse_ss_users(s: &str) -> (String, Option<u32>) {
    // users:(("firefox",pid=1234,fd=55))
    let name = s
        .split("((\"")
        .nth(1)
        .and_then(|r| r.split('"').next())
        .unwrap_or("")
        .to_string();
    let pid = s
        .split("pid=")
        .nth(1)
        .and_then(|r| r.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|p| p.parse().ok());
    (name, pid)
}

/// `lsof -i -n -P`: COMMAND PID USER FD TYPE DEVICE SIZE/OFF NODE NAME
pub fn parse_lsof(text: &str) -> Vec<Connection> {
    text.lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() < 9 {
                return None;
            }
            let name = f[8..].join(" ");
            let (addrs, state) = match name.split_once(" (") {
                Some((a, s)) => (a.to_string(), s.trim_end_matches(')').to_string()),
                None => (name.clone(), String::new()),
            };
            let (local, remote) = addrs
                .split_once("->")
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .unwrap_or((addrs, String::new()));
            Some(Connection {
                proto: f[7].into(),
                local,
                remote,
                state,
                pid: f[1].parse().ok(),
                process: f[0].replace("\\x20", " "),
            })
        })
        .collect()
}

/// A command that blocks outgoing traffic to `ip`, using the OS's own
/// elevation prompt (UAC, macOS admin dialog, or polkit on Linux).
pub fn block_command(ip: &str) -> Option<String> {
    let ok = ip
        .chars()
        .all(|c| c.is_ascii_hexdigit() || c == '.' || c == ':');
    if !ok || ip.is_empty() {
        return None;
    }
    Some(if cfg!(windows) {
        format!(
            "powershell -NoProfile -Command \"Start-Process netsh -Verb RunAs -ArgumentList 'advfirewall firewall add rule name=\\\"OrbitOS block {ip}\\\" dir=out action=block remoteip={ip}'\""
        )
    } else if cfg!(target_os = "macos") {
        format!(
            "osascript -e 'do shell script \"echo \\\"block drop out quick to {ip}\\\" | pfctl -a com.orbitos -f - && pfctl -E\" with administrator privileges'"
        )
    } else if crate::exec::which("ufw").is_some() {
        format!("pkexec ufw deny out to {ip}")
    } else {
        format!("pkexec iptables -A OUTPUT -d {ip} -j DROP")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_windows() {
        let t = "  Proto  Local Address          Foreign Address        State           PID\n  TCP    192.168.1.5:50123      140.82.112.4:443       ESTABLISHED     4412\n  UDP    0.0.0.0:5353           *:*                                    1880\n";
        let c = parse_netstat_windows(t);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].pid, Some(4412));
        assert_eq!(c[0].remote_ip().as_deref(), Some("140.82.112.4"));
        assert_eq!(c[1].remote_ip(), None);
    }

    #[test]
    fn parses_ss() {
        let t =
            "tcp ESTAB 0 0 10.0.0.2:41234 151.101.1.69:443 users:((\"firefox\",pid=2210,fd=88))\n";
        let c = parse_ss(t);
        assert_eq!(c[0].process, "firefox");
        assert_eq!(c[0].pid, Some(2210));
    }

    #[test]
    fn parses_lsof() {
        let t = "COMMAND PID USER FD TYPE DEVICE SIZE/OFF NODE NAME\nSafari 512 me 20u IPv4 0x1 0t0 TCP 10.0.0.2:50000->17.253.1.1:443 (ESTABLISHED)\n";
        let c = parse_lsof(t);
        assert_eq!(c[0].process, "Safari");
        assert_eq!(c[0].state, "ESTABLISHED");
        assert_eq!(c[0].remote_ip().as_deref(), Some("17.253.1.1"));
    }

    #[test]
    fn block_rejects_injection() {
        assert!(block_command("1.2.3.4; rm -rf /").is_none());
        assert!(block_command("1.2.3.4").is_some());
    }
}
