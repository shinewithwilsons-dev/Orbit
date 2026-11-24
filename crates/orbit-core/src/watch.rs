//! On-arrival protection: file-system events from the OS (inotify on Linux,
//! FSEvents on macOS, ReadDirectoryChangesW on Windows). Files are handed
//! to Orbit Shield once they've stopped changing for a moment, so a download
//! is checked as soon as it finishes.

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

pub struct LiveWatcher {
    _watcher: RecommendedWatcher,
    rx: Receiver<PathBuf>,
    pending: HashMap<PathBuf, Instant>,
    settle: Duration,
}

const PARTIAL: &[&str] = &["crdownload", "part", "partial", "download", "tmp"];

impl LiveWatcher {
    pub fn new(dirs: &[PathBuf]) -> notify::Result<Self> {
        let (tx, rx) = channel();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            if let Ok(ev) = res {
                if matches!(ev.kind, EventKind::Create(_) | EventKind::Modify(_)) {
                    for p in ev.paths {
                        let _ = tx.send(p);
                    }
                }
            }
        })?;
        for d in dirs {
            watcher.watch(d, RecursiveMode::Recursive)?;
        }
        Ok(Self {
            _watcher: watcher,
            rx,
            pending: HashMap::new(),
            settle: Duration::from_millis(1500),
        })
    }

    /// Files that arrived and have been quiet for a moment.
    pub fn ready(&mut self) -> Vec<PathBuf> {
        while let Ok(p) = self.rx.try_recv() {
            let partial = p
                .extension()
                .map(|e| PARTIAL.contains(&e.to_string_lossy().to_lowercase().as_str()))
                .unwrap_or(false);
            if !partial {
                self.pending.insert(p, Instant::now());
            }
        }
        let now = Instant::now();
        let done: Vec<PathBuf> = self
            .pending
            .iter()
            .filter(|(_, t)| now.duration_since(**t) >= self.settle)
            .map(|(p, _)| p.clone())
            .collect();
        for p in &done {
            self.pending.remove(p);
        }
        done.into_iter().filter(|p| p.is_file()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sees_new_file() {
        let d = std::env::temp_dir().join(format!("orbit-live-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let mut w = LiveWatcher::new(&[d.clone()]).unwrap();
        w.settle = Duration::from_millis(50);
        std::fs::write(d.join("arrived.txt"), "x").unwrap();
        std::fs::write(d.join("big.iso.crdownload"), "x").unwrap();
        let mut got = Vec::new();
        for _ in 0..40 {
            std::thread::sleep(Duration::from_millis(50));
            got.extend(w.ready());
            if !got.is_empty() {
                break;
            }
        }
        assert!(got.iter().any(|p| p.ends_with("arrived.txt")), "{got:?}");
        assert!(!got
            .iter()
            .any(|p| p.to_string_lossy().ends_with("crdownload")));
        std::fs::remove_dir_all(d).ok();
    }
}
