//! Discover and launch installed applications on each host OS.
//!
//! * Linux: `.desktop` entries (system, user, Flatpak, Snap)
//! * macOS: `.app` bundles in /Applications and ~/Applications
//! * Windows: Start Menu shortcuts (`.lnk` / `.url`)

use anyhow::Result;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct App {
    pub name: String,
    /// Bundle, shortcut or `.desktop` file.
    pub source: PathBuf,
    /// Linux only: the cleaned `Exec=` command line.
    pub exec: Option<String>,
}

pub fn discover() -> Vec<App> {
    let mut apps = if cfg!(target_os = "windows") {
        discover_windows()
    } else if cfg!(target_os = "macos") {
        discover_macos()
    } else {
        discover_linux()
    };
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    apps.dedup_by(|a, b| a.name.eq_ignore_ascii_case(&b.name));
    apps
}

pub fn launch(app: &App) -> Result<()> {
    match &app.exec {
        Some(cmd) => crate::exec::spawn_detached(cmd),
        None => crate::exec::open(&app.source.to_string_lossy()),
    }
}

/// Case-insensitive search by name.
pub fn find<'a>(apps: &'a [App], query: &str) -> Vec<&'a App> {
    let q = query.to_lowercase();
    apps.iter()
        .filter(|a| a.name.to_lowercase().contains(&q))
        .collect()
}

// ------------------------------------------------------------- Linux -----

fn discover_linux() -> Vec<App> {
    let mut dirs: Vec<PathBuf> = vec![
        "/usr/share/applications".into(),
        "/usr/local/share/applications".into(),
        "/var/lib/flatpak/exports/share/applications".into(),
        "/var/lib/snapd/desktop/applications".into(),
    ];
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".local/share/applications"));
        dirs.push(home.join(".local/share/flatpak/exports/share/applications"));
    }
    let mut out = Vec::new();
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("desktop") {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(&p) {
                if let Some(app) = parse_desktop_entry(&text, &p) {
                    out.push(app);
                }
            }
        }
    }
    out
}

/// Parse the `[Desktop Entry]` section of a `.desktop` file.
pub fn parse_desktop_entry(text: &str, source: &Path) -> Option<App> {
    let mut in_main = false;
    let (mut name, mut exec) = (None, None);
    let mut hidden = false;
    let mut is_app = true;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
            continue;
        }
        if !in_main {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            match k.trim() {
                "Name" if name.is_none() => name = Some(v.trim().to_string()),
                "Exec" => exec = Some(clean_exec(v)),
                "NoDisplay" | "Hidden" if v.trim() == "true" => hidden = true,
                "Type" => is_app = v.trim() == "Application",
                _ => {}
            }
        }
    }
    if hidden || !is_app {
        return None;
    }
    Some(App {
        name: name?,
        source: source.to_path_buf(),
        exec: Some(exec?),
    })
}

/// Remove `%f`, `%U` and other field codes from an Exec line.
fn clean_exec(exec: &str) -> String {
    let mut out = String::new();
    let mut chars = exec.trim().chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.next() {
                Some('%') => out.push('%'),
                _ => {}
            }
        } else {
            out.push(c);
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ------------------------------------------------------------- macOS -----

fn discover_macos() -> Vec<App> {
    let mut dirs: Vec<PathBuf> = vec![
        "/Applications".into(),
        "/Applications/Utilities".into(),
        "/System/Applications".into(),
        "/System/Applications/Utilities".into(),
    ];
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join("Applications"));
    }
    let mut out = Vec::new();
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) == Some("app") {
                if let Some(stem) = p.file_stem() {
                    out.push(App {
                        name: stem.to_string_lossy().into_owned(),
                        source: p,
                        exec: None,
                    });
                }
            }
        }
    }
    out
}

// ----------------------------------------------------------- Windows -----

fn discover_windows() -> Vec<App> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Ok(pd) = std::env::var("ProgramData") {
        roots.push(PathBuf::from(pd).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Ok(ad) = std::env::var("APPDATA") {
        roots.push(PathBuf::from(ad).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    let mut out = Vec::new();
    for r in roots {
        walk_shortcuts(&r, 0, &mut out);
    }
    out.retain(|a| !a.name.to_lowercase().contains("uninstall"));
    out
}

fn walk_shortcuts(dir: &Path, depth: usize, out: &mut Vec<App>) {
    if depth > 4 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk_shortcuts(&p, depth + 1, out);
        } else if matches!(
            p.extension().and_then(|x| x.to_str()),
            Some("lnk") | Some("url")
        ) {
            if let Some(stem) = p.file_stem() {
                out.push(App {
                    name: stem.to_string_lossy().into_owned(),
                    source: p,
                    exec: None,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_desktop_file() {
        let txt = "[Desktop Entry]\nType=Application\nName=Firefox\nExec=firefox %u\n\n[Desktop Action new]\nName=New Window\nExec=firefox --new-window\n";
        let app = parse_desktop_entry(txt, Path::new("/x/firefox.desktop")).unwrap();
        assert_eq!(app.name, "Firefox");
        assert_eq!(app.exec.as_deref(), Some("firefox"));
    }

    #[test]
    fn skips_hidden() {
        let txt = "[Desktop Entry]\nType=Application\nName=Hidden\nExec=x\nNoDisplay=true\n";
        assert!(parse_desktop_entry(txt, Path::new("/x")).is_none());
    }

    #[test]
    fn find_is_case_insensitive() {
        let apps = vec![App {
            name: "Visual Studio Code".into(),
            source: "/x".into(),
            exec: None,
        }];
        assert_eq!(find(&apps, "studio").len(), 1);
    }
}
