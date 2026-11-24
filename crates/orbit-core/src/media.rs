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
    pub artist: String,
    pub album: String,
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
                let mut item = Item {
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
                    artist: String::new(),
                    album: String::new(),
                };
                if kind == Kind::Audio {
                    if let Some(t) = read_tags(&item.path, false) {
                        if !t.title.is_empty() {
                            item.title = t.title;
                        }
                        item.artist = t.artist;
                        item.album = t.album;
                    }
                }
                out.push(item);
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

// ------------------------------------------------------- tags and art --

#[derive(Debug, Clone, Default)]
pub struct Tags {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: Option<std::time::Duration>,
    /// Embedded cover art (JPEG/PNG bytes).
    pub art: Option<Vec<u8>>,
}

/// Read title/artist/album (and optionally cover art) from an audio file.
pub fn read_tags(path: &Path, with_art: bool) -> Option<Tags> {
    use lofty::file::{AudioFile, TaggedFileExt};
    use lofty::tag::Accessor;
    let tagged = lofty::read_from_path(path).ok()?;
    let mut t = Tags {
        duration: Some(tagged.properties().duration()),
        ..Default::default()
    };
    if let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) {
        t.title = tag.title().map(|s| s.to_string()).unwrap_or_default();
        t.artist = tag.artist().map(|s| s.to_string()).unwrap_or_default();
        t.album = tag.album().map(|s| s.to_string()).unwrap_or_default();
        if with_art {
            t.art = tag.pictures().first().map(|p| p.data().to_vec());
        }
    }
    if with_art && t.art.is_none() {
        // cover.jpg / folder.jpg next to the file.
        let dir = path.parent()?;
        for name in [
            "cover.jpg",
            "cover.png",
            "folder.jpg",
            "Folder.jpg",
            "front.jpg",
            "AlbumArt.jpg",
        ] {
            if let Ok(b) = std::fs::read(dir.join(name)) {
                t.art = Some(b);
                break;
            }
        }
    }
    Some(t)
}

// ---------------------------------------------------------- playlists --

pub fn playlists_dir() -> PathBuf {
    let d = crate::Config::data_dir().join("playlists");
    let _ = std::fs::create_dir_all(&d);
    d
}

pub fn list_playlists() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(playlists_dir())
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let p = e.path();
                    (p.extension()? == "m3u")
                        .then(|| p.file_stem().unwrap().to_string_lossy().to_string())
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort_by_key(|s| s.to_lowercase());
    v
}

pub fn load_playlist(name: &str) -> Vec<PathBuf> {
    std::fs::read_to_string(playlists_dir().join(format!("{}.m3u", crate::office::safe_name(name))))
        .map(|s| {
            s.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default()
}

pub fn save_playlist(name: &str, tracks: &[PathBuf]) -> Result<()> {
    let mut s = String::from("#EXTM3U\n");
    for t in tracks {
        s.push_str(&format!("{}\n", t.display()));
    }
    std::fs::write(
        playlists_dir().join(format!("{}.m3u", crate::office::safe_name(name))),
        s,
    )?;
    Ok(())
}

pub fn add_to_playlist(name: &str, track: &Path) -> Result<()> {
    let mut v = load_playlist(name);
    if !v.iter().any(|p| p == track) {
        v.push(track.to_path_buf());
    }
    save_playlist(name, &v)
}

pub fn delete_playlist(name: &str) -> Result<()> {
    std::fs::remove_file(playlists_dir().join(format!("{}.m3u", crate::office::safe_name(name))))?;
    Ok(())
}

// ---------------------------------------------------- external players --

#[derive(Debug, Clone)]
pub struct ExternalPlayer {
    pub name: &'static str,
    pub path: PathBuf,
}

pub fn find_mpv() -> Option<PathBuf> {
    crate::exec::which("mpv")
        .or_else(|| crate::exec::which("mpvnet"))
        .or_else(|| {
            crate::exec::first_existing(&[
                r"C:\Program Files\mpv\mpv.exe",
                r"C:\Program Files\mpv.net\mpvnet.exe",
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
        "winget install mpv.net   (or: winget install VideoLAN.VLC)"
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
        if p.name == "mpv" && !p.path.to_string_lossy().contains("mpvnet") {
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
pub use audio::{AudioPlayer, EqGains};

#[cfg(feature = "audio")]
mod audio {
    use anyhow::{Context, Result};
    use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    /// Equaliser gains in dB: bass (120 Hz shelf), mid (1 kHz), treble (8 kHz shelf).
    #[derive(Clone, Default)]
    pub struct EqGains(Arc<[AtomicU32; 3]>);

    impl EqGains {
        pub fn set(&self, g: [f32; 3]) {
            for (a, v) in self.0.iter().zip(g) {
                a.store(v.clamp(-12.0, 12.0).to_bits(), Ordering::Relaxed);
            }
        }
        pub fn get(&self) -> [f32; 3] {
            [0, 1, 2].map(|i| f32::from_bits(self.0[i].load(Ordering::Relaxed)))
        }
    }

    /// RBJ biquad filter.
    #[derive(Clone, Copy, Default)]
    struct Biquad {
        b0: f32,
        b1: f32,
        b2: f32,
        a1: f32,
        a2: f32,
        x1: f32,
        x2: f32,
        y1: f32,
        y2: f32,
    }

    #[derive(Clone, Copy)]
    enum Shape {
        LowShelf,
        Peak,
        HighShelf,
    }

    impl Biquad {
        fn design(shape: Shape, freq: f32, gain_db: f32, rate: f32) -> Self {
            let a = 10f32.powf(gain_db / 40.0);
            let w0 = 2.0 * std::f32::consts::PI * freq / rate;
            let (cos, sin) = (w0.cos(), w0.sin());
            let q = 0.8;
            let alpha = sin / (2.0 * q);
            let (b0, b1, b2, a0, a1, a2) = match shape {
                Shape::Peak => (
                    1.0 + alpha * a,
                    -2.0 * cos,
                    1.0 - alpha * a,
                    1.0 + alpha / a,
                    -2.0 * cos,
                    1.0 - alpha / a,
                ),
                Shape::LowShelf => {
                    let s = 2.0 * a.sqrt() * alpha;
                    (
                        a * ((a + 1.0) - (a - 1.0) * cos + s),
                        2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                        a * ((a + 1.0) - (a - 1.0) * cos - s),
                        (a + 1.0) + (a - 1.0) * cos + s,
                        -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                        (a + 1.0) + (a - 1.0) * cos - s,
                    )
                }
                Shape::HighShelf => {
                    let s = 2.0 * a.sqrt() * alpha;
                    (
                        a * ((a + 1.0) + (a - 1.0) * cos + s),
                        -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                        a * ((a + 1.0) + (a - 1.0) * cos - s),
                        (a + 1.0) - (a - 1.0) * cos + s,
                        2.0 * ((a - 1.0) - (a + 1.0) * cos),
                        (a + 1.0) - (a - 1.0) * cos - s,
                    )
                }
            };
            Self {
                b0: b0 / a0,
                b1: b1 / a0,
                b2: b2 / a0,
                a1: a1 / a0,
                a2: a2 / a0,
                ..Default::default()
            }
        }
        fn retune(&mut self, other: Biquad) {
            self.b0 = other.b0;
            self.b1 = other.b1;
            self.b2 = other.b2;
            self.a1 = other.a1;
            self.a2 = other.a2;
        }
        fn run(&mut self, x: f32) -> f32 {
            let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
                - self.a1 * self.y1
                - self.a2 * self.y2;
            self.x2 = self.x1;
            self.x1 = x;
            self.y2 = self.y1;
            self.y1 = y;
            y
        }
    }

    const BANDS: [(Shape, f32); 3] = [
        (Shape::LowShelf, 120.0),
        (Shape::Peak, 1000.0),
        (Shape::HighShelf, 8000.0),
    ];

    /// 3-band equaliser wrapped around any audio source.
    struct Equalizer<S> {
        inner: S,
        gains: EqGains,
        current: [f32; 3],
        filters: Vec<[Biquad; 3]>,
        channel: usize,
        counter: u32,
    }

    impl<S: Source<Item = f32>> Equalizer<S> {
        fn new(inner: S, gains: EqGains) -> Self {
            let ch = inner.channels().max(1) as usize;
            let mut e = Self {
                inner,
                gains,
                current: [f32::NAN; 3],
                filters: vec![[Biquad::default(); 3]; ch],
                channel: 0,
                counter: 0,
            };
            e.update();
            e
        }
        fn update(&mut self) {
            let g = self.gains.get();
            if g == self.current {
                return;
            }
            self.current = g;
            let rate = self.inner.sample_rate() as f32;
            for chan in self.filters.iter_mut() {
                for (i, (shape, f)) in BANDS.iter().enumerate() {
                    chan[i].retune(Biquad::design(*shape, *f, g[i], rate));
                }
            }
        }
    }

    impl<S: Source<Item = f32>> Iterator for Equalizer<S> {
        type Item = f32;
        fn next(&mut self) -> Option<f32> {
            let x = self.inner.next()?;
            self.counter += 1;
            if self.counter % 2048 == 0 {
                self.update();
            }
            let flat = self.current.iter().all(|g| g.abs() < 0.05);
            let ch = self.channel % self.filters.len();
            self.channel = (self.channel + 1) % self.filters.len();
            if flat {
                return Some(x);
            }
            let mut y = x;
            for f in self.filters[ch].iter_mut() {
                y = f.run(y);
            }
            Some(y.clamp(-1.0, 1.0))
        }
    }

    impl<S: Source<Item = f32>> Source for Equalizer<S> {
        fn current_frame_len(&self) -> Option<usize> {
            self.inner.current_frame_len()
        }
        fn channels(&self) -> u16 {
            self.inner.channels()
        }
        fn sample_rate(&self) -> u32 {
            self.inner.sample_rate()
        }
        fn total_duration(&self) -> Option<Duration> {
            self.inner.total_duration()
        }
        fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
            self.inner.try_seek(pos)
        }
    }

    /// A simple queue-based music player. Not `Send`: keep it on the UI thread.
    pub struct AudioPlayer {
        _stream: OutputStream,
        handle: OutputStreamHandle,
        sink: Sink,
        pub queue: Vec<PathBuf>,
        pub index: Option<usize>,
        pub total: Option<Duration>,
        volume: f32,
        pub eq: EqGains,
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
                eq: EqGains::default(),
            })
        }

        fn load(&mut self, path: &Path) -> Result<()> {
            self.sink.stop();
            self.sink = Sink::try_new(&self.handle)?;
            self.sink.set_volume(self.volume);
            let file = std::fs::File::open(path)?;
            let src = Decoder::new(std::io::BufReader::new(file))
                .context("unsupported or damaged audio file")?;
            self.total = src
                .total_duration()
                .or_else(|| super::read_tags(path, false).and_then(|t| t.duration));
            self.sink.append(Equalizer::new(
                src.convert_samples::<f32>(),
                self.eq.clone(),
            ));
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
    fn playlists() {
        let name = format!("test-{}", std::process::id());
        add_to_playlist(&name, Path::new("/music/a.mp3")).unwrap();
        add_to_playlist(&name, Path::new("/music/b.mp3")).unwrap();
        add_to_playlist(&name, Path::new("/music/a.mp3")).unwrap();
        assert_eq!(load_playlist(&name).len(), 2);
        assert!(list_playlists().contains(&crate::office::safe_name(&name)));
        delete_playlist(&name).unwrap();
    }

    #[test]
    fn durations() {
        assert_eq!(fmt_duration(std::time::Duration::from_secs(125)), "2:05");
    }
}
