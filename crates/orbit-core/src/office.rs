//! Office: integration with LibreOffice, the free open-source office suite
//! (github.com/LibreOffice/core). Create and open documents, convert to
//! PDF / Word, and turn Claude-written drafts into real documents.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocKind {
    Writer,
    Calc,
    Impress,
    Draw,
}

impl DocKind {
    pub const ALL: [DocKind; 4] = [Self::Writer, Self::Calc, Self::Impress, Self::Draw];

    pub fn label(self) -> &'static str {
        match self {
            Self::Writer => "Document",
            Self::Calc => "Spreadsheet",
            Self::Impress => "Presentation",
            Self::Draw => "Drawing",
        }
    }

    pub fn app(self) -> &'static str {
        match self {
            Self::Writer => "Writer",
            Self::Calc => "Calc",
            Self::Impress => "Impress",
            Self::Draw => "Draw",
        }
    }

    fn flag(self) -> &'static str {
        match self {
            Self::Writer => "--writer",
            Self::Calc => "--calc",
            Self::Impress => "--impress",
            Self::Draw => "--draw",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "writer" | "doc" | "document" | "word" => Some(Self::Writer),
            "calc" | "sheet" | "spreadsheet" | "excel" => Some(Self::Calc),
            "impress" | "slides" | "presentation" | "powerpoint" => Some(Self::Impress),
            "draw" | "drawing" => Some(Self::Draw),
            _ => None,
        }
    }
}

pub const DOC_EXT: &[&str] = &[
    "odt", "ods", "odp", "odg", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "rtf", "csv", "pdf",
    "txt", "md",
];

pub fn find_soffice() -> Option<PathBuf> {
    crate::exec::which("soffice")
        .or_else(|| crate::exec::which("libreoffice"))
        .or_else(|| {
            crate::exec::first_existing(&[
                r"C:\Program Files\LibreOffice\program\soffice.exe",
                r"C:\Program Files (x86)\LibreOffice\program\soffice.exe",
                "/Applications/LibreOffice.app/Contents/MacOS/soffice",
                "/opt/libreoffice/program/soffice",
                "/snap/bin/libreoffice",
            ])
        })
}

pub fn install_hint() -> &'static str {
    if cfg!(windows) {
        "winget install TheDocumentFoundation.LibreOffice"
    } else if cfg!(target_os = "macos") {
        "brew install --cask libreoffice"
    } else {
        "sudo apt install libreoffice   (or: flatpak install flathub org.libreoffice.LibreOffice)"
    }
}

fn soffice() -> Result<PathBuf> {
    find_soffice().with_context(|| {
        format!(
            "LibreOffice is not installed. Install it with: {}",
            install_hint()
        )
    })
}

/// Open a blank document in the matching LibreOffice app.
pub fn new_document(kind: DocKind) -> Result<()> {
    Command::new(soffice()?)
        .arg(kind.flag())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

/// Open a file in LibreOffice (or the system default if it's missing).
pub fn open(path: &Path) -> Result<()> {
    match find_soffice() {
        Some(bin) => {
            Command::new(bin)
                .arg(path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
            Ok(())
        }
        None => crate::exec::open(&path.to_string_lossy()),
    }
}

/// Convert `path` to `format` (pdf, docx, odt, xlsx, …) next to the file.
pub fn convert(path: &Path, format: &str) -> Result<PathBuf> {
    convert_with(path, format, None)
}

fn convert_with(path: &Path, format: &str, infilter: Option<&str>) -> Result<PathBuf> {
    let outdir = path.parent().context("file has no folder")?;
    let mut c = Command::new(soffice()?);
    c.arg("--headless");
    if let Some(f) = infilter {
        c.arg(format!("--infilter={f}"));
    }
    let out = c
        .args(["--convert-to", format, "--outdir"])
        .arg(outdir)
        .arg(path)
        .output()?;
    let ext = format.split(':').next().unwrap_or(format);
    let target = path.with_extension(ext);
    if !target.exists() {
        bail!(
            "conversion failed: {}{}",
            String::from_utf8_lossy(&out.stderr).trim(),
            if String::from_utf8_lossy(&out.stderr).is_empty() {
                "(is LibreOffice already open? close it and retry)"
            } else {
                ""
            }
        );
    }
    Ok(target)
}

/// Folder where Claude drafts are saved: Documents/OrbitOS.
pub fn drafts_dir() -> PathBuf {
    crate::user_dirs::documents()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| ".".into())
        .join("OrbitOS")
}

/// Save Claude-written text (simple Markdown) as a Word document.
/// Falls back to `.md` if LibreOffice isn't installed.
pub fn save_draft(title: &str, markdown: &str, format: &str) -> Result<PathBuf> {
    let dir = drafts_dir();
    std::fs::create_dir_all(&dir)?;
    let stem = safe_name(title);
    if find_soffice().is_none() {
        let p = dir.join(format!("{stem}.md"));
        std::fs::write(&p, markdown)?;
        return Ok(p);
    }
    let html = dir.join(format!("{stem}.html"));
    std::fs::write(&html, markdown_to_html(title, markdown))?;
    let result = convert_with(&html, format, Some("HTML (StarWriter)"));
    let _ = std::fs::remove_file(&html);
    result
}

/// Recent office files in Documents and Downloads, newest first.
pub fn recent_documents(limit: usize) -> Vec<PathBuf> {
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    let mut stack: Vec<(PathBuf, usize)> = [
        crate::user_dirs::documents(),
        crate::user_dirs::downloads(),
        crate::user_dirs::desktop(),
    ]
    .into_iter()
    .flatten()
    .map(|d| (d, 0))
    .collect();
    while let Some((d, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            let Ok(m) = e.metadata() else { continue };
            if m.is_dir() {
                if depth < 3 && !e.file_name().to_string_lossy().starts_with('.') {
                    stack.push((p, depth + 1));
                }
            } else if p
                .extension()
                .map(|x| DOC_EXT.contains(&x.to_string_lossy().to_lowercase().as_str()))
                .unwrap_or(false)
            {
                found.push((m.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH), p));
            }
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.into_iter().take(limit).map(|(_, p)| p).collect()
}

pub fn safe_name(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' {
                c
            } else {
                ' '
            }
        })
        .collect();
    let joined = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let short: String = joined.chars().take(60).collect();
    if short.is_empty() {
        "Untitled".into()
    } else {
        short
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Inline **bold** and *italic*.
fn inline(s: &str) -> String {
    let mut out = String::new();
    let mut bold = false;
    let mut italic = false;
    let chars: Vec<char> = esc(s).chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '*' && chars.get(i + 1) == Some(&'*') {
            out.push_str(if bold { "</b>" } else { "<b>" });
            bold = !bold;
            i += 2;
        } else if chars[i] == '*' {
            out.push_str(if italic { "</i>" } else { "<i>" });
            italic = !italic;
            i += 1;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    if bold {
        out.push_str("</b>");
    }
    if italic {
        out.push_str("</i>");
    }
    out
}

/// Minimal Markdown to HTML: headings, bullets, numbered lists, paragraphs.
pub fn markdown_to_html(title: &str, md: &str) -> String {
    let mut body = String::new();
    let mut list: Option<&str> = None;
    let close = |body: &mut String, list: &mut Option<&str>| {
        if let Some(tag) = list.take() {
            body.push_str(&format!("</{tag}>\n"));
        }
    };
    for raw in md.lines() {
        let line = raw.trim_end();
        let t = line.trim_start();
        if t.starts_with("```") {
            continue;
        }
        let level = t.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&level) && t[level..].starts_with(' ') {
            close(&mut body, &mut list);
            body.push_str(&format!(
                "<h{level}>{}</h{level}>\n",
                inline(t[level..].trim())
            ));
        } else if let Some(item) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
            if list != Some("ul") {
                close(&mut body, &mut list);
                body.push_str("<ul>\n");
                list = Some("ul");
            }
            body.push_str(&format!("<li>{}</li>\n", inline(item)));
        } else if let Some((num, item)) = t
            .split_once(". ")
            .filter(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        {
            let _ = num;
            if list != Some("ol") {
                close(&mut body, &mut list);
                body.push_str("<ol>\n");
                list = Some("ol");
            }
            body.push_str(&format!("<li>{}</li>\n", inline(item)));
        } else if t.is_empty() {
            close(&mut body, &mut list);
        } else {
            close(&mut body, &mut list);
            body.push_str(&format!("<p>{}</p>\n", inline(t)));
        }
    }
    close(&mut body, &mut list);
    format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>{}</title></head><body>\n{body}</body></html>\n",
        esc(title)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown() {
        let h = markdown_to_html(
            "T",
            "# Title\nSome **bold** text.\n\n- one\n- two\n1. first\n2. second",
        );
        assert!(h.contains("<h1>Title</h1>"));
        assert!(h.contains("<b>bold</b>"));
        assert!(h.contains("<ul>\n<li>one</li>\n<li>two</li>\n</ul>"));
        assert!(h.contains("<ol>\n<li>first</li>"));
    }

    #[test]
    fn names() {
        assert_eq!(safe_name("My: Report/2026?"), "My Report 2026");
        assert_eq!(safe_name("???"), "Untitled");
        assert_eq!(DocKind::parse("Excel"), Some(DocKind::Calc));
    }
}
