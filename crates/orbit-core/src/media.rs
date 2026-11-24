//! Media: library scanning, in-app music playback, and video through the
//! best free open-source players:
//! * **mpv** (github.com/mpv-player/mpv): preferred for video
//! * **VLC** (github.com/videolan/vlc): fallback
//!
//! Music plays inside OrbitOS (pure Rust, via `rodio` + `symphonia`) when
//! the `audio` feature is enabled.

use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const AUDIO_EXT: &[&str] = &[
    "mp3", "flac", "wav", "ogg", "oga", "opus", "m4a", "aac", "alac", "aiff",
];
pub const VIDEO_EXT: &[&str] = &[
    "mp4", "mkv", "webm", "avi", "mov", "m4v", "wmv", "flv", "mpg", "mpeg", "ts",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Audio,
    Video,
}

#[derive(Debug, Clone)]
pub struct Item {
    pub title: String,
    pub folder: String,
    pub path: PathBuf,
    pub size: u64,
}

pub fn kind_of(path: &Path) -> Option<Kind> {
    let ext = path.extension()?.to_string_lossy().to_lowercase();
    if AUDIO_EXT.contains(&ext.as_str()) {
        Some(Kind::Audio)
    } else if VIDEO_EXT.contains(&ext.as_str()) {
        Some(Kind::Video)
    } else {
        None
    }
}

/// Default library folders for a kind (plus Downloads).
pub fn library_dirs(kind: Kind) -> Vec<PathBuf> {
    let main = match kind {
        Kind::Audio => crate::user_dirs::music(),
        Kind::Video => crate::user_dirs::videos(),
    };
    [main, crate::user_dirs::downloads()]
        .into_iter()
        .flatten()
        .collect()
}

/// Find media files under `dirs` (up to 6 levels deep, 10,000 files).
pub fn scan(dirs: &[PathBuf], kind: Kind) -> Vec<Item> {
    let mut out = Vec::new();
    let mut stack: Vec<(PathBuf, usize)> = dirs.iter().map(|d| (d.clone(), 0)).collect();
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let Ok(m) = e.metadata() else { continue };
            if m.is_dir() {
                if depth < 6 {
                    stack.push((p, depth + 1));
                }
            } else if kind_of(&p) == Some(kind) {
                out.push(Item {
                    title: p
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or(name),
                    folder: dir
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default(),
                    path: p,
                    size: m.len(),
                });
                if out.len() >= 10_000 {
                    return out;
                }
            }
        }
    }
    out.sort_by(|a, b| {
        a.folder
            .to_lowercase()
            .cmp(&b.folder.to_lowercase())
            .then(a.title.to_lowercase().cmp(&b.title.to_lowercase()))
    });
    out.dedup_by(|a, b| a.path == b.path);
    out
}

// ---------------------------------------------------- external players --

#[derive(Debug, Clone)]
pub struct ExternalPlayer {
    pub name: &'static str,
    pub path: PathBuf,
}

pub fn find_mpv() -> Option<PathBuf> {
    crate::exec::which("mpv").or_else(|| {
        crate::exec::first_existing(&[
            r"C:\Program Files\mpv\mpv.exe",
            "~/scoop/apps/mpv/current/mpv.exe",
            "/Applications/mpv.app/Contents/MacOS/mpv",
            "/opt/homebrew/bin/mpv",
            "/usr/local/bin/mpv",
        ])
    })
}

pub fn find_vlc() -> Option<PathBuf> {
    crate::exec::which("vlc").or_else(|| {
        crate::exec::first_existing(&[
            r"C:\Program Files\VideoLAN\VLC\vlc.exe",
            r"C:\Program Files (x86)\VideoLAN\VLC\vlc.exe",
            "/Applications/VLC.app/Contents/MacOS/VLC",
        ])
    })
}

/// Installed players, best first.
pub fn players() -> Vec<ExternalPlayer> {
    let mut v = Vec::new();
    if let Some(p) = find_mpv() {
        v.push(ExternalPlayer {
            name: "mpv",
            path: p,
        });
    }
    if let Some(p) = find_vlc() {
        v.push(ExternalPlayer {
            name: "VLC",
            path: p,
        });
    }
    v
}

pub fn player_install_hint() -> &'static str {
    if cfg!(windows) {
        "winget install mpv   (or: winget install VideoLAN.VLC)"
    } else if cfg!(target_os = "macos") {
        "brew install mpv   (or: brew install --cask vlc)"
    } else {
        "sudo apt install mpv   (or: sudo apt install vlc)"
    }
}

/// Play a file (or several, as a playlist) in mpv, then VLC, then the
/// system default app.
pub fn play_external(paths: &[PathBuf]) -> Result<&'static str> {
    if paths.is_empty() {
        anyhow::bail!("nothing to play");
    }
    for p in players() {
        let mut c = Command::new(&p.path);
        if p.name == "mpv" {
            c.args([
                "--force-window=yes",
                "--keep-open=no",
                "--title=OrbitOS Media · ${media-title}",
            ]);
        }
        c.args(paths)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if c.spawn().is_ok() {
            return Ok(p.name);
        }
    }
    crate::exec::open(&paths[0].to_string_lossy())?;
    Ok("system default player")
}

// ------------------------------------------------------ in-app audio ----

#[cfg(feature = "audio")]
pub use audio::AudioPlayer;

#[cfg(feature = "audio")]
mod audio {
    use anyhow::{Context, Result};
    use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    /// A simple queue-based music player. Not `Send`: keep it on the UI thread.
    pub struct AudioPlayer {
        _stream: OutputStream,
        handle: OutputStreamHandle,
        sink: Sink,
        pub queue: Vec<PathBuf>,
        pub index: Option<usize>,
        pub total: Option<Duration>,
        volume: f32,
    }

    impl AudioPlayer {
        pub fn new(volume: f32) -> Result<Self> {
            let (stream, handle) = OutputStream::try_default().context("no audio output device")?;
            let sink = Sink::try_new(&handle)?;
            sink.set_volume(volume);
            Ok(Self {
                _stream: stream,
                handle,
                sink,
                queue: Vec::new(),
                index: None,
                total: None,
                volume,
            })
        }

        fn load(&mut self, path: &Path) -> Result<()> {
            self.sink.stop();
            self.sink = Sink::try_new(&self.handle)?;
            self.sink.set_volume(self.volume);
            let file = std::fs::File::open(path)?;
            let src = Decoder::new(std::io::BufReader::new(file))
                .context("unsupported or damaged audio file")?;
            self.total = src.total_duration();
            self.sink.append(src);
            self.sink.play();
            Ok(())
        }

        /// Replace the queue and start at `start`.
        pub fn play_queue(&mut self, queue: Vec<PathBuf>, start: usize) -> Result<()> {
            self.queue = queue;
            self.play_index(start)
        }

        pub fn play_index(&mut self, i: usize) -> Result<()> {
            let path = self.queue.get(i).cloned().context("track not in queue")?;
            self.index = Some(i);
            self.load(&path)
        }

        pub fn toggle(&self) {
            if self.sink.is_paused() {
                self.sink.play()
            } else {
                self.sink.pause()
            }
        }

        pub fn stop(&mut self) {
            self.sink.stop();
            self.index = None;
            self.total = None;
        }

        pub fn next(&mut self) -> Result<()> {
            match self.index {
                Some(i) if i + 1 < self.queue.len() => self.play_index(i + 1),
                _ => {
                    self.stop();
                    Ok(())
                }
            }
        }

        pub fn prev(&mut self) -> Result<()> {
            match self.index {
                Some(i) if self.position() < Duration::from_secs(3) && i > 0 => {
                    self.play_index(i - 1)
                }
                Some(i) => self.play_index(i),
                None => Ok(()),
            }
        }

        pub fn seek(&self, to: Duration) {
            let _ = self.sink.try_seek(to);
        }

        pub fn set_volume(&mut self, v: f32) {
            self.volume = v.clamp(0.0, 1.0);
            self.sink.set_volume(self.volume);
        }

        pub fn is_paused(&self) -> bool {
            self.sink.is_paused()
        }

        pub fn position(&self) -> Duration {
            self.sink.get_pos()
        }

        pub fn current(&self) -> Option<&PathBuf> {
            self.index.and_then(|i| self.queue.get(i))
        }

        /// Call regularly: moves to the next track when one finishes.
        /// Returns true if the track changed.
        pub fn tick(&mut self) -> bool {
            if self.index.is_some() && self.sink.empty() {
                let _ = self.next();
                return true;
            }
            false
        }
    }
}

pub fn fmt_duration(d: std::time::Duration) -> String {
    let s = d.as_secs();
    format!("{}:{:02}", s / 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds() {
        assert_eq!(kind_of(Path::new("a/b.MP3")), Some(Kind::Audio));
        assert_eq!(kind_of(Path::new("film.mkv")), Some(Kind::Video));
        assert_eq!(kind_of(Path::new("x.txt")), None);
    }

    #[test]
    fn scans_library() {
        let d = std::env::temp_dir().join(format!("orbit-media-{}", std::process::id()));
        std::fs::create_dir_all(d.join("Album")).unwrap();
        std::fs::write(d.join("Album").join("song.flac"), b"x").unwrap();
        std::fs::write(d.join("clip.mp4"), b"x").unwrap();
        assert_eq!(scan(&[d.clone()], Kind::Audio).len(), 1);
        assert_eq!(scan(&[d.clone()], Kind::Video)[0].title, "clip");
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn durations() {
        assert_eq!(fmt_duration(std::time::Duration::from_secs(125)), "2:05");
    }
}
