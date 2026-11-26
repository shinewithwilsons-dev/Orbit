//! Hardware setup: scan the computer for devices that have no driver, a
//! driver that failed, or are switched off, and suggest a fix for each.
//!
//! Scanning only reads. Every fix is a command that OrbitOS shows the user
//! and runs only after they approve it. Windows and Linux get a per-device
//! check; macOS manages drivers through the system, so it reports pending
//! software and firmware updates instead.

use crate::exec;
use serde_json::Value;
use std::path::PathBuf;

/// What the scan found for one device.
#[derive(Debug, Clone, PartialEq)]
pub enum State {
    Ok,
    /// Windows: no driver installed. Linux: no kernel driver loaded.
    Missing,
    /// Windows: switched off in Device Manager.
    Disabled,
    /// Any other error the operating system reports.
    Problem(String),
}

#[derive(Debug, Clone)]
pub struct Device {
    pub class: String,
    pub name: String,
    pub maker: String,
    pub state: State,
    /// Windows instance id, or the PCI address on Linux.
    pub id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Info,
    Warn,
    Bad,
}

/// A command that fixes a finding. OrbitOS asks before running it.
#[derive(Debug, Clone)]
pub struct Fix {
    pub label: String,
    pub command: String,
}

#[derive(Debug, Clone)]
pub struct Finding {
    pub level: Level,
    pub title: String,
    pub detail: String,
    pub fix: Option<Fix>,
}

#[derive(Debug, Clone, Default)]
pub struct Report {
    pub system: String,
    pub devices: Vec<Device>,
    /// Most serious first.
    pub findings: Vec<Finding>,
    /// Checks that could not run, and why.
    pub notes: Vec<String>,
}

/// Scan this computer. Slow (it runs system tools), so call it off the UI thread.
pub fn scan() -> Report {
    let mut r = Report {
        system: system_line(),
        ..Default::default()
    };
    if cfg!(windows) {
        scan_windows(&mut r);
        device_findings(&mut r, true);
    } else if cfg!(target_os = "macos") {
        scan_macos(&mut r);
    } else {
        scan_linux(&mut r);
        device_findings(&mut r, false);
    }
    r.findings.sort_by_key(|f| std::cmp::Reverse(f.level));
    r
}

/// A web search for a device's driver, for the "Find driver" button.
pub fn driver_search_url(device: &Device) -> String {
    let query: String = format!("{} {} driver", device.maker, device.name)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("+")
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '+' || *c == '-' || *c == '.')
        .collect();
    format!("https://duckduckgo.com/?q={query}")
}

fn cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| std::env::temp_dir())
}

fn system_line() -> String {
    use sysinfo::System;
    let mut s = System::new();
    s.refresh_cpu();
    s.refresh_memory();
    let cpu = s
        .cpus()
        .first()
        .map(|c| c.brand().trim().to_string())
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| "Unknown processor".into());
    let gb = s.total_memory() as f64 / 1_073_741_824.0;
    let os = System::long_os_version().unwrap_or_else(|| std::env::consts::OS.into());
    format!("{os} · {cpu} · {gb:.0} GB RAM")
}

// ------------------------------------------------------------ findings ----

/// Turn the device list into findings, with one fix where a single command covers many devices.
fn device_findings(r: &mut Report, windows: bool) {
    let missing: Vec<&Device> = r.devices.iter().filter(|d| d.state == State::Missing).collect();
    if !missing.is_empty() {
        let names: Vec<&str> = missing.iter().take(4).map(|d| d.name.as_str()).collect();
        let more = if missing.len() > 4 { " and more" } else { "" };
        let fix = if windows {
            Some(Fix {
                label: "Search Windows Update for drivers".into(),
                command: "UsoClient StartScan".into(),
            })
        } else if exec::which("ubuntu-drivers").is_some() {
            Some(Fix {
                label: "Install recommended drivers".into(),
                command: "pkexec ubuntu-drivers install".into(),
            })
        } else if exec::which("apt-get").is_some() {
            Some(Fix {
                label: "Install firmware packages".into(),
                command: "pkexec apt-get install -y linux-firmware".into(),
            })
        } else {
            None
        };
        r.findings.push(Finding {
            level: Level::Bad,
            title: format!("{} device(s) without a driver", missing.len()),
            detail: format!(
                "{}{more}. Use Find driver beside each one, or let the system search for it.",
                names.join(", ")
            ),
            fix,
        });
    }

    for d in r.devices.iter().filter(|d| d.state == State::Disabled).take(5) {
        r.findings.push(Finding {
            level: Level::Warn,
            title: format!("{} is switched off", d.name),
            detail: "Windows has this device disabled.".into(),
            fix: Some(Fix {
                label: "Turn it on".into(),
                command: format!("pnputil /enable-device \"{}\"", d.id),
            }),
        });
    }

    for d in r.devices.iter().filter(|d| matches!(d.state, State::Problem(_))).take(5) {
        let State::Problem(p) = &d.state else { continue };
        r.findings.push(Finding {
            level: Level::Bad,
            title: format!("{} is reporting an error", d.name),
            detail: format!("{p}. Restarting the device often clears this."),
            fix: windows.then(|| Fix {
                label: "Restart device".into(),
                command: format!("pnputil /restart-device \"{}\"", d.id),
            }),
        });
    }

    if windows {
        r.findings.push(Finding {
            level: Level::Info,
            title: "Rescan hardware".into(),
            detail: "Ask Windows to look again for devices it has not set up.".into(),
            fix: Some(Fix {
                label: "Rescan devices".into(),
                command: "pnputil /scan-devices".into(),
            }),
        });
    }
}

// ------------------------------------------------------------- windows ----

const WINDOWS_DEVICES: &str = "Get-PnpDevice -PresentOnly | Select-Object Class,FriendlyName,Manufacturer,InstanceId,@{n='Problem';e={[int]$_.Problem}} | ConvertTo-Json -Compress";

fn scan_windows(r: &mut Report) {
    let cmd = format!("powershell -NoProfile -ExecutionPolicy Bypass -Command \"{WINDOWS_DEVICES}\"");
    let out = match exec::run_capture(&cmd, &cwd()) {
        Ok(o) => o,
        Err(e) => {
            r.notes.push(format!("Could not read the device list: {e}"));
            return;
        }
    };
    let Ok(value) = serde_json::from_str::<Value>(out.stdout.trim()) else {
        r.notes.push("Windows did not return a device list.".into());
        return;
    };
    let rows = match value {
        Value::Array(a) => a,
        obj @ Value::Object(_) => vec![obj],
        _ => Vec::new(),
    };
    for d in rows {
        let text = |k: &str| {
            d.get(k)
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string()
        };
        let state = match d.get("Problem").and_then(Value::as_i64).unwrap_or(0) {
            0 => State::Ok,
            28 => State::Missing,
            22 => State::Disabled,
            n => State::Problem(format!("Windows error code {n}")),
        };
        let name = text("FriendlyName");
        r.devices.push(Device {
            class: text("Class"),
            name: if name.is_empty() { "Unnamed device".into() } else { name },
            maker: text("Manufacturer"),
            state,
            id: text("InstanceId"),
        });
    }
}

// --------------------------------------------------------------- linux ----

/// Device classes whose missing driver stops something a person uses.
const USER_CLASSES: [&str; 8] = [
    "VGA", "3D", "Display", "Network", "Ethernet", "Wireless", "Audio", "Multimedia",
];

fn scan_linux(r: &mut Report) {
    if exec::which("lspci").is_none() {
        r.notes.push("lspci is not installed, so devices could not be listed. Install pciutils to enable the device scan.".into());
        return;
    }
    match exec::run_capture("lspci -k", &cwd()) {
        Ok(out) => parse_lspci(&out.stdout, &mut r.devices),
        Err(e) => r.notes.push(format!("lspci failed: {e}")),
    }
}

/// Parse `lspci -k`. Each device starts on an unindented line; its indented
/// lines include `Kernel driver in use:` when a driver is loaded.
fn parse_lspci(text: &str, out: &mut Vec<Device>) {
    let mut blocks: Vec<(String, bool)> = Vec::new();
    for line in text.lines() {
        if line.starts_with(char::is_whitespace) {
            if line.trim_start().starts_with("Kernel driver in use:") {
                if let Some(last) = blocks.last_mut() {
                    last.1 = true;
                }
            }
        } else if !line.trim().is_empty() {
            blocks.push((line.to_string(), false));
        }
    }
    for (header, has_driver) in blocks {
        // "00:02.0 VGA compatible controller: Intel Corporation ..."
        let Some((addr_class, name)) = header.split_once(": ") else {
            continue;
        };
        let mut parts = addr_class.splitn(2, ' ');
        let addr = parts.next().unwrap_or("").to_string();
        let class = parts.next().unwrap_or("").to_string();
        let needed = USER_CLASSES.iter().any(|c| class.contains(c));
        let state = if needed && !has_driver {
            State::Missing
        } else {
            State::Ok
        };
        out.push(Device {
            class,
            name: name.trim().to_string(),
            maker: String::new(),
            state,
            id: addr,
        });
    }
}

// --------------------------------------------------------------- macos ----

fn scan_macos(r: &mut Report) {
    match exec::run_capture("softwareupdate -l", &cwd()) {
        Ok(out) => {
            let pending: Vec<&str> = out
                .stdout
                .lines()
                .chain(out.stderr.lines())
                .filter_map(|l| l.trim().strip_prefix("* Label:"))
                .map(str::trim)
                .collect();
            if !pending.is_empty() {
                let shown = pending.iter().take(3).copied().collect::<Vec<_>>().join(", ");
                let more = if pending.len() > 3 { " and more" } else { "" };
                r.findings.push(Finding {
                    level: Level::Warn,
                    title: format!("{} system update(s) waiting", pending.len()),
                    detail: format!("{shown}{more}. These can include driver and firmware updates."),
                    fix: Some(Fix {
                        label: "Install all updates (asks for your password)".into(),
                        command: "softwareupdate -i -a".into(),
                    }),
                });
            }
        }
        Err(e) => r.notes.push(format!("Could not check for updates: {e}")),
    }
    r.notes.push("macOS installs drivers through its own updates, so there is no per-device driver list here.".into());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lspci_flags_user_device_without_driver() {
        let sample = "00:02.0 VGA compatible controller: Intel Corporation UHD Graphics\n\tSubsystem: Dell\n\tKernel driver in use: i915\n\n02:00.0 Network controller: Realtek Semiconductor RTL8852\n\tSubsystem: Foo\n\n00:1f.0 ISA bridge: Intel Corporation LPC\n";
        let mut devices = Vec::new();
        parse_lspci(sample, &mut devices);
        assert_eq!(devices.len(), 3);
        assert_eq!(devices[0].state, State::Ok);
        assert_eq!(devices[1].state, State::Missing);
        assert_eq!(devices[1].id, "02:00.0");
        // Bridges are not user-facing, so a missing driver there is not flagged.
        assert_eq!(devices[2].state, State::Ok);
    }

    #[test]
    fn search_url_is_encoded() {
        let d = Device {
            class: String::new(),
            name: "Wi-Fi 6 AX201 & Bluetooth".into(),
            maker: "Intel".into(),
            state: State::Missing,
            id: String::new(),
        };
        let url = driver_search_url(&d);
        assert!(url.starts_with("https://duckduckgo.com/?q=Intel+Wi-Fi+6+AX201"));
        assert!(!url.contains('&'));
        assert!(!url.contains(' '));
    }
}
