//! Claude actions: Claude can propose real actions in ```action blocks.
//! OrbitOS shows each one as an approval card and only runs it after the
//! user approves. There is deliberately no delete tool.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case")]
pub enum Action {
    RunCommand {
        command: String,
    },
    OpenApp {
        name: String,
    },
    OpenPath {
        path: String,
    },
    ListFolder {
        path: String,
    },
    MakeFolder {
        path: String,
    },
    MoveFile {
        from: String,
        to: String,
    },
    CopyFile {
        from: String,
        to: String,
    },
    WriteFile {
        path: String,
        content: String,
    },
    Scan {
        path: String,
    },
    Quarantine {
        path: String,
        #[serde(default)]
        reason: String,
    },
    PlayMedia {
        path: String,
    },
    CreateDocument {
        title: String,
        markdown: String,
        #[serde(default)]
        format: String,
    },
    GenerateImage {
        prompt: String,
    },
    SetTheme {
        theme: String,
    },
    SetWallpaper {
        wallpaper: String,
    },
    Notify {
        title: String,
        #[serde(default)]
        body: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Risk {
    /// Reads or shows something.
    Safe,
    /// Changes files or settings.
    Changes,
    /// Runs an arbitrary command.
    Command,
}

/// Effects only the desktop can perform.
#[derive(Debug, Clone)]
pub enum UiEffect {
    SetTheme(String),
    SetWallpaper(String),
    Notify(String, String),
    PlayMedia(PathBuf),
    OpenFolder(PathBuf),
    ShowImage(PathBuf),
}

#[derive(Debug, Clone)]
pub struct Outcome {
    pub ok: bool,
    pub message: String,
    pub effect: Option<UiEffect>,
}

/// Instructions added to Claude's system prompt.
pub fn tool_guide() -> &'static str {
    "You can also ACT on this computer. To act, add one fenced block per action tagged `action` \
containing a JSON object, for example:\n\
```action\n{\"tool\": \"move_file\", \"from\": \"~/Downloads/report.pdf\", \"to\": \"~/Documents/report.pdf\"}\n```\n\
Tools: run_command{command} · open_app{name} · open_path{path} (file, folder or URL) · \
list_folder{path} · make_folder{path} · move_file{from,to} · copy_file{from,to} · \
write_file{path,content} (new files only) · scan{path} (antivirus) · quarantine{path,reason} · \
play_media{path} · create_document{title,markdown,format: docx|odt|pdf} · generate_image{prompt} · \
set_theme{theme: orbit|light|mars|ocean|mono} · set_wallpaper{wallpaper: starfield|warp|planet|aurora|plain} · \
notify{title,body}.\n\
Every action is shown to the user for approval first. There is no delete tool: if something should \
be deleted, suggest quarantine or explain how. Use ~ for the home folder. Briefly say what you're \
about to do before the action blocks. After the user approves, results are sent back to you."
}

impl Action {
    pub fn risk(&self) -> Risk {
        match self {
            Action::ListFolder { .. }
            | Action::OpenPath { .. }
            | Action::OpenApp { .. }
            | Action::Scan { .. }
            | Action::PlayMedia { .. }
            | Action::Notify { .. } => Risk::Safe,
            Action::RunCommand { .. } => Risk::Command,
            _ => Risk::Changes,
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            Action::RunCommand { .. } => "Run command",
            Action::OpenApp { .. } => "Open app",
            Action::OpenPath { .. } => "Open",
            Action::ListFolder { .. } => "Look in folder",
            Action::MakeFolder { .. } => "Create folder",
            Action::MoveFile { .. } => "Move",
            Action::CopyFile { .. } => "Copy",
            Action::WriteFile { .. } => "Create file",
            Action::Scan { .. } => "Scan with Orbit Shield",
            Action::Quarantine { .. } => "Quarantine",
            Action::PlayMedia { .. } => "Play",
            Action::CreateDocument { .. } => "Create document",
            Action::GenerateImage { .. } => "Generate image",
            Action::SetTheme { .. } => "Change theme",
            Action::SetWallpaper { .. } => "Change wallpaper",
            Action::Notify { .. } => "Notification",
        }
    }

    /// One-line description for the approval card.
    pub fn detail(&self) -> String {
        match self {
            Action::RunCommand { command } => command.clone(),
            Action::OpenApp { name } => name.clone(),
            Action::OpenPath { path }
            | Action::ListFolder { path }
            | Action::MakeFolder { path }
            | Action::Scan { path }
            | Action::PlayMedia { path } => path.clone(),
            Action::MoveFile { from, to } | Action::CopyFile { from, to } => {
                format!("{from} -> {to}")
            }
            Action::WriteFile { path, content } => {
                format!("{path} ({} characters)", content.chars().count())
            }
            Action::Quarantine { path, reason } => {
                if reason.is_empty() {
                    path.clone()
                } else {
                    format!("{path} ({reason})")
                }
            }
            Action::CreateDocument { title, format, .. } => {
                format!(
                    "\"{title}\" as .{}",
                    if format.is_empty() { "docx" } else { format }
                )
            }
            Action::GenerateImage { prompt } => prompt.clone(),
            Action::SetTheme { theme } => theme.clone(),
            Action::SetWallpaper { wallpaper } => wallpaper.clone(),
            Action::Notify { title, body } => format!("{title}: {body}"),
        }
    }
}

/// Find every ```action block. Bad JSON is returned as an error string.
pub fn extract(text: &str) -> Vec<std::result::Result<Action, String>> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("```action") {
        let after = &rest[start + 9..];
        let body_start = after.find('\n').map(|i| i + 1).unwrap_or(after.len());
        let body = &after[body_start..];
        let Some(end) = body.find("```") else { break };
        let json = body[..end].trim();
        out.push(
            serde_json::from_str::<Action>(json)
                .map_err(|e| format!("couldn't read action ({e}): {json}")),
        );
        rest = &body[end + 3..];
    }
    out
}

fn resolve(p: &str, cwd: &Path) -> PathBuf {
    let p = crate::expand_tilde(p.trim());
    if p.is_absolute() {
        p
    } else {
        cwd.join(p)
    }
}

fn ok(message: impl Into<String>, effect: Option<UiEffect>) -> Outcome {
    Outcome {
        ok: true,
        message: message.into(),
        effect,
    }
}

/// Run an approved action.
pub fn execute(action: &Action, cfg: &crate::Config, cwd: &Path) -> Outcome {
    match run(action, cfg, cwd) {
        Ok(o) => o,
        Err(e) => Outcome {
            ok: false,
            message: format!("{}: {e}", action.title()),
            effect: None,
        },
    }
}

fn run(action: &Action, cfg: &crate::Config, cwd: &Path) -> Result<Outcome> {
    Ok(match action {
        Action::RunCommand { command } => {
            let out = crate::exec::run_capture(command, cwd)?;
            let mut text = format!("{}{}", out.stdout, out.stderr);
            if text.chars().count() > 4000 {
                text = format!(
                    "{}… (truncated)",
                    text.chars().take(4000).collect::<String>()
                );
            }
            Outcome {
                ok: out.status.success(),
                message: format!("$ {command}\n{text}"),
                effect: None,
            }
        }
        Action::OpenApp { name } => {
            let apps = crate::apps::discover();
            let found = crate::apps::find(&apps, name);
            let app = found
                .iter()
                .find(|a| a.name.eq_ignore_ascii_case(name))
                .or_else(|| found.first())
                .with_context(|| format!("no installed app matches '{name}'"))?;
            crate::apps::launch(app)?;
            ok(format!("Opened {}", app.name), None)
        }
        Action::OpenPath { path } => {
            if path.starts_with("http://") || path.starts_with("https://") {
                crate::exec::open(path)?;
                return Ok(ok(format!("Opened {path}"), None));
            }
            let p = resolve(path, cwd);
            if p.is_dir() {
                return Ok(ok(
                    format!("Opened folder {}", p.display()),
                    Some(UiEffect::OpenFolder(p)),
                ));
            }
            crate::exec::open(&p.to_string_lossy())?;
            ok(format!("Opened {}", p.display()), None)
        }
        Action::ListFolder { path } => {
            let p = resolve(path, cwd);
            let entries = crate::files::list(&p, false)?;
            let mut s = format!("{} ({} items):\n", p.display(), entries.len());
            for e in entries.iter().take(200) {
                if e.is_dir {
                    s.push_str(&format!("  {}/\n", e.name));
                } else {
                    s.push_str(&format!("  {}  ({})\n", e.name, crate::human_bytes(e.size)));
                }
            }
            ok(s, None)
        }
        Action::MakeFolder { path } => {
            let p = resolve(path, cwd);
            std::fs::create_dir_all(&p)?;
            ok(format!("Created {}", p.display()), None)
        }
        Action::MoveFile { from, to } => {
            let (f, t) = (resolve(from, cwd), resolve(to, cwd));
            let t = if t.is_dir() {
                t.join(f.file_name().context("bad source")?)
            } else {
                t
            };
            if t.exists() {
                bail!("{} already exists; I won't overwrite it", t.display());
            }
            if let Some(parent) = t.parent() {
                std::fs::create_dir_all(parent)?;
            }
            if std::fs::rename(&f, &t).is_err() {
                // Different drive: copy then remove.
                if f.is_dir() {
                    bail!("moving folders between drives isn't supported");
                }
                std::fs::copy(&f, &t)?;
                std::fs::remove_file(&f)?;
            }
            ok(format!("Moved {} -> {}", f.display(), t.display()), None)
        }
        Action::CopyFile { from, to } => {
            let (f, t) = (resolve(from, cwd), resolve(to, cwd));
            let t = if t.is_dir() {
                t.join(f.file_name().context("bad source")?)
            } else {
                t
            };
            if t.exists() {
                bail!("{} already exists; I won't overwrite it", t.display());
            }
            if let Some(parent) = t.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&f, &t)?;
            ok(format!("Copied to {}", t.display()), None)
        }
        Action::WriteFile { path, content } => {
            let p = resolve(path, cwd);
            if p.exists() {
                bail!("{} already exists; I only create new files", p.display());
            }
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&p, content)?;
            ok(format!("Created {}", p.display()), None)
        }
        Action::Scan { path } => {
            let p = resolve(path, cwd);
            let opts = crate::antivirus::ScanOptions {
                use_clamav: cfg.use_clamav,
                ..Default::default()
            };
            let r = crate::antivirus::scan(
                &[p.clone()],
                &opts,
                &std::sync::atomic::AtomicBool::new(false),
                &mut |_, _| {},
            );
            let mut s = format!(
                "Scanned {} files in {}: {} finding(s)\n",
                r.files_scanned,
                p.display(),
                r.findings.len()
            );
            for f in &r.findings {
                s.push_str(&format!(
                    "  [{}] {} - {}\n",
                    f.severity.label(),
                    f.threat,
                    f.path.display()
                ));
            }
            ok(s, None)
        }
        Action::Quarantine { path, reason } => {
            let p = resolve(path, cwd);
            crate::antivirus::quarantine(
                &p,
                if reason.is_empty() {
                    "Quarantined by Claude (user approved)"
                } else {
                    reason
                },
            )?;
            ok(format!("Quarantined {}", p.display()), None)
        }
        Action::PlayMedia { path } => {
            let p = if path.starts_with("http") {
                PathBuf::from(path)
            } else {
                resolve(path, cwd)
            };
            ok(
                format!("Playing {}", p.display()),
                Some(UiEffect::PlayMedia(p)),
            )
        }
        Action::CreateDocument {
            title,
            markdown,
            format,
        } => {
            let fmt = if format.is_empty() {
                "docx"
            } else {
                format.as_str()
            };
            let p = crate::office::save_draft(title, markdown, fmt)?;
            let _ = crate::office::open(&p);
            ok(format!("Created {}", p.display()), None)
        }
        Action::GenerateImage { prompt } => {
            let p = crate::claude::generate_image(cfg, prompt)?;
            ok(
                format!("Saved image {}", p.display()),
                Some(UiEffect::ShowImage(p)),
            )
        }
        Action::SetTheme { theme } => ok(
            format!("Theme set to {theme}"),
            Some(UiEffect::SetTheme(theme.clone())),
        ),
        Action::SetWallpaper { wallpaper } => ok(
            format!("Wallpaper set to {wallpaper}"),
            Some(UiEffect::SetWallpaper(wallpaper.clone())),
        ),
        Action::Notify { title, body } => ok(
            "Notification shown",
            Some(UiEffect::Notify(title.clone(), body.clone())),
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_actions() {
        let t = "Sure.\n```action\n{\"tool\":\"make_folder\",\"path\":\"~/x\"}\n```\n```action\n{\"tool\":\"nope\"}\n```";
        let a = extract(t);
        assert_eq!(a.len(), 2);
        assert_eq!(
            a[0].as_ref().unwrap(),
            &Action::MakeFolder { path: "~/x".into() }
        );
        assert!(a[1].is_err());
    }

    #[test]
    fn file_actions_never_overwrite() {
        let d = std::env::temp_dir().join(format!("orbit-act-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let cfg = crate::Config::default();
        let w = Action::WriteFile {
            path: "a.txt".into(),
            content: "hi".into(),
        };
        assert!(execute(&w, &cfg, &d).ok);
        assert!(
            !execute(&w, &cfg, &d).ok,
            "second write must refuse to overwrite"
        );
        let m = Action::MoveFile {
            from: "a.txt".into(),
            to: "sub/b.txt".into(),
        };
        assert!(execute(&m, &cfg, &d).ok);
        assert!(d.join("sub/b.txt").exists() && !d.join("a.txt").exists());
        let l = execute(&Action::ListFolder { path: "sub".into() }, &cfg, &d);
        assert!(l.message.contains("b.txt"));
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn risk_levels() {
        assert_eq!(
            Action::RunCommand {
                command: "ls".into()
            }
            .risk(),
            Risk::Command
        );
        assert_eq!(Action::ListFolder { path: ".".into() }.risk(), Risk::Safe);
        assert_eq!(
            Action::MoveFile {
                from: "a".into(),
                to: "b".into()
            }
            .risk(),
            Risk::Changes
        );
    }
}
