//! Orbit Shield: OrbitOS's built-in antivirus.
//!
//! Two engines work together:
//! * **Orbit engine** (built in, pure Rust): SHA-256 signature matching,
//!   the EICAR test file, dangerous file-name tricks (double extensions,
//!   right-to-left override) and script heuristics (ransomware commands,
//!   download-and-execute, obfuscated PowerShell).
//! * **ClamAV** (open source, github.com/Cisco-Talos/clamav): used
//!   automatically when `clamscan` is installed, for millions of signatures.
//!
//! Suspicious files can be quarantined: they are moved into OrbitOS's data
//! folder and XOR-encoded so they can't be run by accident, and can be
//! restored later.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Severity {
    Low,
    Medium,
    High,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub path: PathBuf,
    pub threat: String,
    pub engine: String,
    pub severity: Severity,
    pub detail: String,
}

impl Finding {
    /// Text for asking Claude about this finding.
    pub fn describe(&self) -> String {
        format!(
            "Orbit Shield flagged a file.\nFile: {}\nThreat: {}\nEngine: {}\nSeverity: {}\nWhy: {}\n\n\
             Explain in plain English what this is, how dangerous it is, and whether I should \
             quarantine it, delete it, or ignore it as a false positive.",
            self.path.display(),
            self.threat,
            self.engine,
            self.severity.label(),
            self.detail
        )
    }
}

#[derive(Debug, Clone, Default)]
pub struct ScanReport {
    pub findings: Vec<Finding>,
    pub files_scanned: usize,
    pub skipped: usize,
    pub duration: Duration,
    pub clamav_used: bool,
    pub notes: Vec<String>,
    pub cancelled: bool,
}

#[derive(Debug, Clone)]
pub struct ScanOptions {
    pub max_file_size: u64,
    pub use_clamav: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            max_file_size: 150 * 1024 * 1024,
            use_clamav: true,
        }
    }
}

// ------------------------------------------------------------ engines ----

/// The standard antivirus test string (harmless; every AV detects it).
/// Stored XOR-encoded so OrbitOS's own binary isn't flagged by other scanners.
const EICAR_ENC: &[u8] = &[
    2, 111, 21, 123, 10, 127, 26, 27, 10, 1, 110, 6, 10, 0, 2, 111, 110, 114, 10, 4, 115, 109, 25,
    25, 115, 109, 39, 126, 31, 19, 25, 27, 8, 119, 9, 14, 27, 20, 30, 27, 8, 30, 119, 27, 20, 14,
    19, 12, 19, 8, 15, 9, 119, 14, 31, 9, 14, 119, 28, 19, 22, 31, 123, 126, 18, 113, 18, 112,
];

fn eicar() -> Vec<u8> {
    EICAR_ENC.iter().map(|b| b ^ 0x5A).collect()
}

/// SHA-256 signatures built into OrbitOS. Add more in `signatures.txt`.
const BUILTIN_SIGNATURES: &[(&str, &str)] = &[(
    "275a021bbfb6489e54d471899f7db9d1663fc695ec2fe2a2c4538aabf651fd0f",
    "EICAR-Test-File",
)];

const EXEC_EXT: &[&str] = &[
    "exe", "scr", "com", "bat", "cmd", "js", "jse", "vbs", "vbe", "ps1", "lnk", "pif", "msi",
    "jar", "hta", "wsf", "app", "sh",
];
const DECOY_EXT: &[&str] = &[
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "jpg", "jpeg", "png", "gif", "txt", "mp3",
    "mp4", "zip", "rar",
];
const SCRIPT_EXT: &[&str] = &[
    "ps1", "bat", "cmd", "vbs", "js", "jse", "hta", "wsf", "sh", "command",
];

/// Script rules: (all of these substrings in group A, any in group B, threat, severity).
struct Rule {
    all: &'static [&'static str],
    any: &'static [&'static str],
    threat: &'static str,
    severity: Severity,
    why: &'static str,
}

const RULES: &[Rule] = &[
    Rule {
        all: &["vssadmin", "delete", "shadows"],
        any: &[""],
        threat: "Heuristic.Ransomware.ShadowDelete",
        severity: Severity::High,
        why: "deletes Windows backup snapshots, a classic ransomware step",
    },
    Rule {
        all: &["bcdedit", "recoveryenabled", "no"],
        any: &[""],
        threat: "Heuristic.Ransomware.DisableRecovery",
        severity: Severity::High,
        why: "turns off Windows recovery mode",
    },
    Rule {
        all: &["wbadmin", "delete", "catalog"],
        any: &[""],
        threat: "Heuristic.Ransomware.BackupDelete",
        severity: Severity::High,
        why: "deletes the Windows backup catalog",
    },
    Rule {
        all: &[],
        any: &[
            "downloadstring(",
            "downloadfile(",
            "invoke-webrequest",
            "net.webclient",
        ],
        threat: "",
        severity: Severity::Low,
        why: "",
    },
    Rule {
        all: &["frombase64string("],
        any: &["invoke-expression", "iex(", "iex ("],
        threat: "Heuristic.PowerShell.EncodedExec",
        severity: Severity::Medium,
        why: "decodes hidden code and runs it",
    },
    Rule {
        all: &["powershell"],
        any: &[
            "-encodedcommand",
            " -enc ",
            " -e jab",
            "-windowstyle hidden",
        ],
        threat: "Heuristic.PowerShell.Obfuscated",
        severity: Severity::Medium,
        why: "launches PowerShell with hidden or encoded commands",
    },
    Rule {
        all: &[],
        any: &["| sh", "|sh", "| bash", "|bash"],
        threat: "",
        severity: Severity::Low,
        why: "",
    },
];

pub struct Engines {
    signatures: HashMap<String, String>,
    pub clamav: Option<PathBuf>,
}

impl Engines {
    pub fn load() -> Self {
        let mut signatures: HashMap<String, String> = BUILTIN_SIGNATURES
            .iter()
            .map(|(h, n)| (h.to_string(), n.to_string()))
            .collect();
        if let Ok(text) = std::fs::read_to_string(signatures_path()) {
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let mut parts = line.splitn(2, char::is_whitespace);
                if let (Some(h), name) = (parts.next(), parts.next()) {
                    if h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit()) {
                        signatures.insert(
                            h.to_lowercase(),
                            name.unwrap_or("User-Signature").trim().to_string(),
                        );
                    }
                }
            }
        }
        Self {
            signatures,
            clamav: find_clamscan(),
        }
    }

    pub fn signature_count(&self) -> usize {
        self.signatures.len()
    }

    /// Check one file with the Orbit engine.
    pub fn check_file(&self, path: &Path, max_size: u64) -> Vec<Finding> {
        let mut out = Vec::new();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let lower = name.to_lowercase();
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();

        // File-name tricks.
        if name.contains('\u{202E}') {
            out.push(finding(
                path,
                "Trick.RightToLeftOverride",
                Severity::High,
                "the name hides its real extension using a right-to-left character",
            ));
        }
        let parts: Vec<&str> = lower.split('.').collect();
        if parts.len() >= 3 {
            let last = parts[parts.len() - 1];
            let prev = parts[parts.len() - 2];
            if EXEC_EXT.contains(&last) && DECOY_EXT.contains(&prev) {
                out.push(finding(
                    path,
                    "Trick.DoubleExtension",
                    Severity::High,
                    &format!("looks like a .{prev} file but is really a .{last} program"),
                ));
            }
        }

        let Ok(meta) = std::fs::metadata(path) else {
            return out;
        };
        if meta.len() == 0 || meta.len() > max_size {
            return out;
        }

        // Signatures + EICAR.
        if let Ok((hash, head)) = hash_file(path) {
            if let Some(name) = self.signatures.get(&hash) {
                out.push(finding(
                    path,
                    name,
                    Severity::High,
                    &format!("matches known-bad SHA-256 {hash}"),
                ));
            } else if contains(&head, &eicar()) {
                out.push(finding(
                    path,
                    "EICAR-Test-File",
                    Severity::High,
                    "contains the standard antivirus test string (harmless, used to test scanners)",
                ));
            }
        }

        // Script heuristics.
        if SCRIPT_EXT.contains(&ext.as_str()) {
            if let Ok(text) = read_lower(path, 512 * 1024) {
                let downloads = matches(&text, &RULES[3]);
                let pipes_shell = matches(&text, &RULES[6]);
                let runs = [
                    "invoke-expression",
                    "iex(",
                    "iex (",
                    "start-process",
                    "| iex",
                ]
                .iter()
                .any(|k| text.contains(k));
                for (i, r) in RULES.iter().enumerate() {
                    if i == 3 || i == 6 {
                        continue;
                    }
                    if matches(&text, r) {
                        out.push(finding(path, r.threat, r.severity, r.why));
                    }
                }
                if downloads && runs {
                    out.push(finding(
                        path,
                        "Heuristic.Script.DownloadExecute",
                        Severity::Medium,
                        "downloads code from the internet and runs it straight away",
                    ));
                }
                if pipes_shell && (text.contains("curl ") || text.contains("wget ")) {
                    out.push(finding(
                        path,
                        "Heuristic.Script.PipeToShell",
                        Severity::Low,
                        "downloads a script and pipes it straight into a shell",
                    ));
                }
            }
        }
        out
    }
}

fn matches(text: &str, r: &Rule) -> bool {
    r.all.iter().all(|k| text.contains(k)) && r.any.iter().any(|k| k.is_empty() || text.contains(k))
}

fn finding(path: &Path, threat: &str, severity: Severity, why: &str) -> Finding {
    Finding {
        path: path.to_path_buf(),
        threat: threat.to_string(),
        engine: "Orbit engine".into(),
        severity,
        detail: why.to_string(),
    }
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// SHA-256 of the whole file, plus its first 4 KB.
fn hash_file(path: &Path) -> Result<(String, Vec<u8>)> {
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut head = Vec::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        if head.len() < 4096 {
            head.extend_from_slice(&buf[..n.min(4096 - head.len())]);
        }
        hasher.update(&buf[..n]);
    }
    let hash = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok((hash, head))
}

fn read_lower(path: &Path, max: usize) -> Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut buf = vec![0u8; max];
    let n = f.read(&mut buf)?;
    buf.truncate(n);
    Ok(String::from_utf8_lossy(&buf).to_lowercase())
}

// --------------------------------------------------------------- scan ----

/// Folders a quick scan covers: Downloads, Desktop, Documents and temp.
pub fn quick_scan_paths() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = [
        crate::user_dirs::downloads(),
        crate::user_dirs::desktop(),
        crate::user_dirs::documents(),
    ]
    .into_iter()
    .flatten()
    .collect();
    v.push(std::env::temp_dir());
    v.retain(|p| p.exists());
    v.dedup();
    v
}

/// Folders watched by real-time protection.
pub fn watch_paths() -> Vec<PathBuf> {
    [crate::user_dirs::downloads(), crate::user_dirs::desktop()]
        .into_iter()
        .flatten()
        .collect()
}

const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    ".cache",
    "$Recycle.Bin",
    "System Volume Information",
];

/// Scan files and folders. `progress` gets the count so far and the
/// current file. Set `cancel` to stop early.
pub fn scan(
    targets: &[PathBuf],
    opts: &ScanOptions,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(usize, &Path),
) -> ScanReport {
    let start = Instant::now();
    let engines = Engines::load();
    let mut report = ScanReport::default();
    let quarantine = quarantine_dir();

    let mut stack: Vec<PathBuf> = targets.to_vec();
    while let Some(p) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            break;
        }
        if p.starts_with(&quarantine) {
            continue;
        }
        let Ok(meta) = std::fs::symlink_metadata(&p) else {
            report.skipped += 1;
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            let skip = p
                .file_name()
                .map(|n| SKIP_DIRS.contains(&n.to_string_lossy().as_ref()))
                .unwrap_or(false);
            if skip {
                continue;
            }
            match std::fs::read_dir(&p) {
                Ok(rd) => stack.extend(rd.flatten().map(|e| e.path())),
                Err(_) => report.skipped += 1,
            }
            continue;
        }
        report.files_scanned += 1;
        if report.files_scanned % 25 == 0 || report.files_scanned < 25 {
            progress(report.files_scanned, &p);
        }
        report
            .findings
            .extend(engines.check_file(&p, opts.max_file_size));
    }

    if opts.use_clamav && !report.cancelled {
        match &engines.clamav {
            Some(bin) => match clamav_scan(bin, targets) {
                Ok((found, note)) => {
                    report.clamav_used = true;
                    for f in found {
                        let dup = report
                            .findings
                            .iter()
                            .any(|x| x.path == f.path && x.threat == f.threat);
                        if !dup {
                            report.findings.push(f);
                        }
                    }
                    if let Some(n) = note {
                        report.notes.push(n);
                    }
                }
                Err(e) => report.notes.push(format!("ClamAV: {e}")),
            },
            None => report
                .notes
                .push(format!("ClamAV not installed: {}", clamav_install_hint())),
        }
    }
    report.findings.sort_by(|a, b| b.severity.cmp(&a.severity));
    report.duration = start.elapsed();
    report
}

// ------------------------------------------------------------- ClamAV ----

pub fn find_clamscan() -> Option<PathBuf> {
    crate::exec::which("clamscan").or_else(|| {
        crate::exec::first_existing(&[
            r"C:\Program Files\ClamAV\clamscan.exe",
            "/opt/homebrew/bin/clamscan",
            "/usr/local/bin/clamscan",
        ])
    })
}

pub fn clamav_install_hint() -> &'static str {
    if cfg!(windows) {
        "install from clamav.net/downloads (or `winget install Cisco.ClamAV`)"
    } else if cfg!(target_os = "macos") {
        "`brew install clamav`"
    } else {
        "`sudo apt install clamav` (or your distro's package)"
    }
}

/// ClamAV version string, if installed.
pub fn clamav_version() -> Option<String> {
    let bin = find_clamscan()?;
    let out = Command::new(bin).arg("--version").output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn clamav_scan(bin: &Path, targets: &[PathBuf]) -> Result<(Vec<Finding>, Option<String>)> {
    let out = Command::new(bin)
        .args(["-r", "--infected", "--no-summary", "--stdout"])
        .args(targets)
        .output()
        .context("could not run clamscan")?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let found = parse_clamav_output(&stdout);
    if out.status.code() == Some(2) && found.is_empty() {
        let all = format!("{stdout}{stderr}");
        if all.contains("database") {
            bail!("no virus database yet: use Update signatures in Orbit Shield (or `update-av` in the shell)");
        }
        bail!(
            "{}",
            all.lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("scan error")
        );
    }
    Ok((found, None))
}

/// Parse `path: Threat.Name FOUND` lines.
pub fn parse_clamav_output(text: &str) -> Vec<Finding> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim().strip_suffix(" FOUND")?;
            let idx = l.rfind(": ")?;
            let threat = &l[idx + 2..];
            Some(Finding {
                path: PathBuf::from(&l[..idx]),
                threat: threat.to_string(),
                engine: "ClamAV".into(),
                severity: if threat.starts_with("PUA.") {
                    Severity::Medium
                } else {
                    Severity::High
                },
                detail: "matched a ClamAV signature".into(),
            })
        })
        .collect()
}

/// Download the latest ClamAV signatures (`freshclam`).
pub fn update_clamav() -> Result<String> {
    let bin = crate::exec::which("freshclam")
        .or_else(|| crate::exec::first_existing(&[r"C:\Program Files\ClamAV\freshclam.exe"]))
        .with_context(|| format!("freshclam not found: {}", clamav_install_hint()))?;
    let out = Command::new(bin).output()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if !out.status.success() {
        let hint = if text.to_lowercase().contains("permission") || text.contains("lock") {
            " (on Linux it may need `sudo freshclam`, or the clamav-freshclam service already updates it)"
        } else {
            ""
        };
        bail!(
            "{}{hint}",
            text.lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("freshclam failed")
        );
    }
    Ok(text
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("Up to date")
        .to_string())
}

// --------------------------------------------------------- quarantine ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuarantineItem {
    pub id: String,
    pub original: PathBuf,
    pub threat: String,
    pub quarantined_at: u64,
    pub size: u64,
}

pub fn quarantine_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("orbitos")
        .join("quarantine")
}

pub fn signatures_path() -> PathBuf {
    crate::Config::path().with_file_name("signatures.txt")
}

const XOR_KEY: u8 = 0x5A;

fn xor_copy(from: &Path, to: &Path) -> Result<u64> {
    let mut data = std::fs::read(from)?;
    for b in data.iter_mut() {
        *b ^= XOR_KEY;
    }
    std::fs::write(to, &data)?;
    Ok(data.len() as u64)
}

/// Move a file into quarantine (encoded so it can't run).
pub fn quarantine(path: &Path, threat: &str) -> Result<QuarantineItem> {
    let dir = quarantine_dir();
    std::fs::create_dir_all(&dir)?;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let id = format!("{}-{}", now.as_millis(), std::process::id());
    let size = xor_copy(path, &dir.join(format!("{id}.qbin")))?;
    let item = QuarantineItem {
        id: id.clone(),
        original: path.to_path_buf(),
        threat: threat.to_string(),
        quarantined_at: now.as_secs(),
        size,
    };
    std::fs::write(
        dir.join(format!("{id}.json")),
        serde_json::to_string_pretty(&item)?,
    )?;
    if let Err(e) = std::fs::remove_file(path) {
        // Roll back so we never end up with a copy in both places.
        let _ = std::fs::remove_file(dir.join(format!("{id}.qbin")));
        let _ = std::fs::remove_file(dir.join(format!("{id}.json")));
        bail!("could not remove the original file: {e}");
    }
    Ok(item)
}

pub fn quarantine_list() -> Vec<QuarantineItem> {
    let Ok(rd) = std::fs::read_dir(quarantine_dir()) else {
        return Vec::new();
    };
    let mut items: Vec<QuarantineItem> = rd
        .flatten()
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
        .filter_map(|e| serde_json::from_str(&std::fs::read_to_string(e.path()).ok()?).ok())
        .collect();
    items.sort_by(|a, b| b.quarantined_at.cmp(&a.quarantined_at));
    items
}

/// Put a quarantined file back where it came from.
pub fn restore(id: &str) -> Result<PathBuf> {
    let dir = quarantine_dir();
    let meta = dir.join(format!("{id}.json"));
    let item: QuarantineItem = serde_json::from_str(&std::fs::read_to_string(&meta)?)?;
    if item.original.exists() {
        bail!("a file already exists at {}", item.original.display());
    }
    if let Some(parent) = item.original.parent() {
        std::fs::create_dir_all(parent)?;
    }
    xor_copy(&dir.join(format!("{id}.qbin")), &item.original)?;
    std::fs::remove_file(dir.join(format!("{id}.qbin")))?;
    std::fs::remove_file(meta)?;
    Ok(item.original)
}

/// Permanently delete a quarantined file.
pub fn delete_quarantined(id: &str) -> Result<()> {
    let dir = quarantine_dir();
    std::fs::remove_file(dir.join(format!("{id}.qbin")))?;
    std::fs::remove_file(dir.join(format!("{id}.json")))?;
    Ok(())
}

// ------------------------------------------------------ real-time watch --

/// Polls folders and reports files that are new or changed since last poll.
pub struct Watcher {
    dirs: Vec<PathBuf>,
    seen: HashMap<PathBuf, SystemTime>,
    primed: bool,
}

impl Watcher {
    pub fn new(dirs: Vec<PathBuf>) -> Self {
        Self {
            dirs,
            seen: HashMap::new(),
            primed: false,
        }
    }

    /// New or modified files since the previous call. The first call only
    /// records what's already there.
    pub fn poll(&mut self) -> Vec<PathBuf> {
        let mut changed = Vec::new();
        for d in &self.dirs {
            let Ok(rd) = std::fs::read_dir(d) else {
                continue;
            };
            for e in rd.flatten() {
                let p = e.path();
                let Ok(m) = e.metadata() else { continue };
                if !m.is_file() {
                    continue;
                }
                let mtime = m.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                let prev = self.seen.insert(p.clone(), mtime);
                if self.primed && prev != Some(mtime) {
                    changed.push(p);
                }
            }
        }
        self.primed = true;
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("orbit-av-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn detects_eicar() {
        let d = tmp("eicar");
        let f = d.join("eicar.com");
        std::fs::write(&f, eicar()).unwrap();
        let found = Engines::load().check_file(&f, 1 << 20);
        assert!(
            found.iter().any(|x| x.threat.contains("EICAR")),
            "{found:?}"
        );
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn detects_double_extension_and_ransomware_script() {
        let d = tmp("tricks");
        let a = d.join("invoice.pdf.exe");
        std::fs::write(&a, "MZ").unwrap();
        let b = d.join("x.bat");
        std::fs::write(&b, "vssadmin.exe Delete Shadows /All /Quiet").unwrap();
        let e = Engines::load();
        assert!(e
            .check_file(&a, 1 << 20)
            .iter()
            .any(|x| x.threat == "Trick.DoubleExtension"));
        let fb = e.check_file(&b, 1 << 20);
        assert!(fb
            .iter()
            .any(|x| x.threat == "Heuristic.Ransomware.ShadowDelete"));
        assert_eq!(fb[0].severity, Severity::High);
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn clean_files_are_clean() {
        let d = tmp("clean");
        let f = d.join("notes.txt");
        std::fs::write(&f, "shopping list: eggs, milk").unwrap();
        let s = d.join("build.sh");
        std::fs::write(&s, "#!/bin/sh\ncargo build --release\n").unwrap();
        let e = Engines::load();
        assert!(e.check_file(&f, 1 << 20).is_empty());
        assert!(e.check_file(&s, 1 << 20).is_empty());
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn scan_walks_folders() {
        let d = tmp("walk");
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::write(d.join("sub").join("t.com"), eicar()).unwrap();
        std::fs::write(d.join("ok.txt"), "fine").unwrap();
        let opts = ScanOptions {
            use_clamav: false,
            ..Default::default()
        };
        let r = scan(&[d.clone()], &opts, &AtomicBool::new(false), &mut |_, _| {});
        assert_eq!(r.files_scanned, 2);
        assert_eq!(r.findings.len(), 1);
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn quarantine_roundtrip() {
        let d = tmp("quar");
        let f = d.join("bad.com");
        std::fs::write(&f, eicar()).unwrap();
        let item = quarantine(&f, "EICAR-Test-File").unwrap();
        assert!(!f.exists());
        let stored = std::fs::read(quarantine_dir().join(format!("{}.qbin", item.id))).unwrap();
        assert!(
            !contains(&stored, &eicar()),
            "quarantined copy must be encoded"
        );
        assert!(quarantine_list().iter().any(|q| q.id == item.id));
        restore(&item.id).unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), eicar());
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn parses_clamav() {
        let f = parse_clamav_output(
            "/home/a/x.exe: Win.Trojan.Agent-123 FOUND\n/home/a/y: PUA.Win.Tool FOUND\n",
        );
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].threat, "Win.Trojan.Agent-123");
        assert_eq!(f[1].severity, Severity::Medium);
    }

    #[test]
    fn watcher_sees_new_files() {
        let d = tmp("watch");
        let mut w = Watcher::new(vec![d.clone()]);
        assert!(w.poll().is_empty());
        std::fs::write(d.join("new.txt"), "x").unwrap();
        assert_eq!(w.poll().len(), 1);
        assert!(w.poll().is_empty());
        std::fs::remove_dir_all(d).ok();
    }
}
