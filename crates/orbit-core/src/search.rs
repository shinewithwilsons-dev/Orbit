//! File index for the command bar: a fast in-memory list of files and
//! folders in your home folder, rebuilt in the background.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Hit {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
}

#[derive(Debug, Default, Clone)]
pub struct FileIndex {
    items: Vec<(String, Hit)>,
}

const SKIP: &[&str] = &[
    "node_modules",
    "target",
    ".git",
    "Library",
    "AppData",
    "$Recycle.Bin",
    ".cache",
    "__pycache__",
    "venv",
    ".venv",
];

impl FileIndex {
    /// Index `roots` (up to `max_depth` levels and `max_items` entries).
    pub fn build(roots: &[PathBuf], max_depth: usize, max_items: usize) -> Self {
        let mut items = Vec::new();
        let mut stack: Vec<(PathBuf, usize)> = roots.iter().map(|r| (r.clone(), 0)).collect();
        while let Some((dir, depth)) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&dir) else {
                continue;
            };
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with('.') || SKIP.contains(&name.as_str()) {
                    continue;
                }
                let Ok(ft) = e.file_type() else { continue };
                if ft.is_symlink() {
                    continue;
                }
                let is_dir = ft.is_dir();
                if is_dir && depth < max_depth {
                    stack.push((e.path(), depth + 1));
                }
                items.push((
                    name.to_lowercase(),
                    Hit {
                        name,
                        path: e.path(),
                        is_dir,
                    },
                ));
                if items.len() >= max_items {
                    return Self { items };
                }
            }
        }
        Self { items }
    }

    pub fn home() -> Self {
        let roots: Vec<PathBuf> = dirs::home_dir().into_iter().collect();
        Self::build(&roots, 6, 150_000)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Every word of the query must appear in the name. Names that start
    /// with the query rank first, then shorter names.
    pub fn query(&self, q: &str, limit: usize) -> Vec<Hit> {
        let q = q.trim().to_lowercase();
        if q.len() < 2 {
            return Vec::new();
        }
        let words: Vec<&str> = q.split_whitespace().collect();
        let mut hits: Vec<(u32, &Hit)> = self
            .items
            .iter()
            .filter(|(n, _)| words.iter().all(|w| n.contains(w)))
            .map(|(n, h)| {
                let mut score = n.len() as u32;
                if n.starts_with(&q) {
                    score /= 4;
                }
                if h.is_dir {
                    score = score.saturating_sub(2);
                }
                (score, h)
            })
            .collect();
        hits.sort_by_key(|(s, _)| *s);
        hits.into_iter()
            .take(limit)
            .map(|(_, h)| h.clone())
            .collect()
    }
}

/// Short display form of a path (home shown as ~).
pub fn pretty(p: &Path) -> String {
    if let Some(h) = dirs::home_dir() {
        if let Ok(rest) = p.strip_prefix(&h) {
            return format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display());
        }
    }
    p.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_files() {
        let d = std::env::temp_dir().join(format!("orbit-idx-{}", std::process::id()));
        std::fs::create_dir_all(d.join("Projects/ccwl")).unwrap();
        std::fs::write(d.join("Projects/ccwl/hull design.txt"), "").unwrap();
        std::fs::write(d.join("notes.txt"), "").unwrap();
        let idx = FileIndex::build(&[d.clone()], 5, 1000);
        assert_eq!(idx.len(), 4);
        let h = idx.query("hull des", 5);
        assert_eq!(h[0].name, "hull design.txt");
        assert_eq!(idx.query("ccwl", 5)[0].name, "ccwl");
        assert!(idx.query("x", 5).is_empty());
        std::fs::remove_dir_all(d).ok();
    }
}
