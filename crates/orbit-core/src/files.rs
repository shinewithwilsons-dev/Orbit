//! Simple file-system helpers for the file manager and shell.

use anyhow::Result;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

/// List a directory: folders first, then files, alphabetically.
/// Hidden entries (starting with `.`) are skipped unless `show_hidden`.
pub fn list(dir: &Path, show_hidden: bool) -> Result<Vec<Entry>> {
    let mut out = Vec::new();
    for item in std::fs::read_dir(dir)? {
        let Ok(item) = item else { continue };
        let name = item.file_name().to_string_lossy().into_owned();
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        let meta = item.metadata().ok();
        out.push(Entry {
            name,
            path: item.path(),
            is_dir: meta.as_ref().map(|m| m.is_dir()).unwrap_or(false),
            size: meta.as_ref().map(|m| m.len()).unwrap_or(0),
            modified: meta.and_then(|m| m.modified().ok()),
        });
    }
    out.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(out)
}

/// Read up to `max` bytes of a text file. Returns `None` for binaries.
pub fn read_text_preview(path: &Path, max: usize) -> Option<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; max];
    let n = f.read(&mut buf).ok()?;
    buf.truncate(n);
    if buf.contains(&0) {
        return None;
    }
    Some(String::from_utf8_lossy(&buf).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_dirs_first() {
        let base = std::env::temp_dir().join(format!("orbit-test-{}", std::process::id()));
        std::fs::create_dir_all(base.join("zdir")).unwrap();
        std::fs::write(base.join("a.txt"), "hello").unwrap();
        let entries = list(&base, false).unwrap();
        assert_eq!(entries[0].name, "zdir");
        assert!(entries[0].is_dir);
        assert_eq!(entries[1].name, "a.txt");
        assert_eq!(
            read_text_preview(&base.join("a.txt"), 100).unwrap(),
            "hello"
        );
        std::fs::remove_dir_all(base).ok();
    }
}
