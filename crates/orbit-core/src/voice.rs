//! Voice: push-to-talk and "Hey Orbit", with speech-to-text by whisper.cpp
//! (github.com/ggerganov/whisper.cpp, runs locally and offline) and spoken
//! replies through each OS's built-in speech engine.

use anyhow::{bail, Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// whisper.cpp's command-line program.
pub fn find_whisper(cfg: &crate::Config) -> Option<PathBuf> {
    if let Some(p) = cfg.whisper_bin.as_ref().filter(|p| !p.trim().is_empty()) {
        return Some(crate::expand_tilde(p));
    }
    ["whisper-cli", "whisper-cpp", "whisper.cpp"]
        .iter()
        .find_map(|n| crate::exec::which(n))
        .or_else(|| {
            crate::exec::first_existing(&[
                "~/whisper.cpp/build/bin/whisper-cli",
                "/opt/homebrew/bin/whisper-cli",
                r"C:\Program Files\whisper.cpp\whisper-cli.exe",
            ])
        })
}

/// Folder where OrbitOS looks for whisper models (`ggml-*.bin`).
pub fn models_dir() -> PathBuf {
    crate::Config::data_dir().join("whisper")
}

pub fn find_model(cfg: &crate::Config) -> Option<PathBuf> {
    if let Some(p) = cfg.whisper_model.as_ref().filter(|p| !p.trim().is_empty()) {
        return Some(crate::expand_tilde(p));
    }
    let mut dirs = vec![models_dir()];
    if let Some(h) = dirs::home_dir() {
        dirs.push(h.join("whisper.cpp").join("models"));
    }
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        let mut models: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                let n = p
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                n.starts_with("ggml-") && n.ends_with(".bin")
            })
            .collect();
        // Prefer base/small models: a good balance of speed and accuracy.
        models.sort_by_key(|p| {
            let n = p.to_string_lossy().to_string();
            if n.contains("base") {
                0
            } else if n.contains("small") {
                1
            } else if n.contains("tiny") {
                2
            } else {
                3
            }
        });
        if let Some(m) = models.into_iter().next() {
            return Some(m);
        }
    }
    None
}

pub fn setup_hint() -> String {
    let install = if cfg!(windows) {
        "download whisper.cpp for Windows from github.com/ggerganov/whisper.cpp/releases"
    } else if cfg!(target_os = "macos") {
        "brew install whisper-cpp"
    } else {
        "build whisper.cpp from github.com/ggerganov/whisper.cpp (cmake -B build && cmake --build build)"
    };
    format!(
        "1) Install whisper.cpp: {install}\n2) Download a model such as ggml-base.en.bin from huggingface.co/ggerganov/whisper.cpp into {}",
        models_dir().display()
    )
}

/// Turn a 16 kHz mono WAV file into text.
pub fn transcribe(cfg: &crate::Config, wav: &Path) -> Result<String> {
    let bin = find_whisper(cfg).with_context(setup_hint)?;
    let model = find_model(cfg).with_context(setup_hint)?;
    let out = Command::new(bin)
        .arg("-m")
        .arg(model)
        .arg("-f")
        .arg(wav)
        .args(["-nt", "-np", "-l", "auto"])
        .stdin(Stdio::null())
        .output()
        .context("could not run whisper.cpp")?;
    if !out.status.success() {
        bail!(
            "whisper.cpp failed: {}",
            String::from_utf8_lossy(&out.stderr)
                .lines()
                .last()
                .unwrap_or("")
        );
    }
    Ok(clean_transcript(&String::from_utf8_lossy(&out.stdout)))
}

/// Remove whisper's markers like [BLANK_AUDIO] and (music).
pub fn clean_transcript(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = (depth - 1).max(0),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

const WAKE: &[&str] = &[
    "hey orbit",
    "hi orbit",
    "okay orbit",
    "ok orbit",
    "hey orbits",
    "a orbit",
];

fn norm(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn has_wake_word(text: &str) -> bool {
    let n = norm(text);
    WAKE.iter().any(|w| n.contains(w))
}

/// Text after the wake word ("hey orbit, play music" -> "play music").
pub fn after_wake_word(text: &str) -> String {
    let n = norm(text);
    for w in WAKE {
        if let Some(i) = n.find(w) {
            return n[i + w.len()..].trim().to_string();
        }
    }
    n
}

/// Strip Markdown and code so replies sound natural when read aloud.
pub fn speakable(text: &str) -> String {
    let mut out = String::new();
    let mut in_code = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        let l = line
            .trim_start_matches(['#', '-', '*', '>', ' '])
            .replace("**", "")
            .replace('`', "");
        if !l.trim().is_empty() {
            out.push_str(l.trim());
            out.push(' ');
        }
    }
    let s: String = out.chars().take(700).collect();
    s.trim().to_string()
}

/// Speak text with the OS voice. Returns the process so it can be stopped.
pub fn speak(text: &str) -> Result<Child> {
    let text = speakable(text);
    if text.is_empty() {
        bail!("nothing to say");
    }
    let mut child = if cfg!(windows) {
        Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "Add-Type -AssemblyName System.Speech; (New-Object System.Speech.Synthesis.SpeechSynthesizer).Speak([Console]::In.ReadToEnd())",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?
    } else if cfg!(target_os = "macos") {
        Command::new("say")
            .args(["-f", "-"])
            .stdin(Stdio::piped())
            .spawn()?
    } else if let Some(espeak) =
        crate::exec::which("espeak-ng").or_else(|| crate::exec::which("espeak"))
    {
        Command::new(espeak)
            .arg("--stdin")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?
    } else if let Some(spd) = crate::exec::which("spd-say") {
        return Ok(Command::new(spd)
            .args(["-w", &text])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?);
    } else {
        bail!("no speech engine found: install espeak-ng (sudo apt install espeak-ng)");
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
    }
    Ok(child)
}

/// Write 16 kHz mono samples to a WAV file.
pub fn write_wav(samples: &[f32], path: &Path) -> Result<()> {
    let mut data = Vec::with_capacity(44 + samples.len() * 2);
    let len = (samples.len() * 2) as u32;
    data.extend_from_slice(b"RIFF");
    data.extend_from_slice(&(36 + len).to_le_bytes());
    data.extend_from_slice(b"WAVEfmt ");
    data.extend_from_slice(&16u32.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes()); // PCM
    data.extend_from_slice(&1u16.to_le_bytes()); // mono
    data.extend_from_slice(&16_000u32.to_le_bytes());
    data.extend_from_slice(&32_000u32.to_le_bytes());
    data.extend_from_slice(&2u16.to_le_bytes());
    data.extend_from_slice(&16u16.to_le_bytes());
    data.extend_from_slice(b"data");
    data.extend_from_slice(&len.to_le_bytes());
    for s in samples {
        data.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    std::fs::write(path, data)?;
    Ok(())
}

/// Loudness of a chunk (root mean square).
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

/// Linear resampling to 16 kHz.
pub fn resample_16k(input: &[f32], rate: u32) -> Vec<f32> {
    if rate == 16_000 || input.is_empty() {
        return input.to_vec();
    }
    let ratio = rate as f64 / 16_000.0;
    let n = (input.len() as f64 / ratio) as usize;
    (0..n)
        .map(|i| {
            let pos = i as f64 * ratio;
            let j = pos as usize;
            let frac = (pos - j as f64) as f32;
            let a = input[j.min(input.len() - 1)];
            let b = input[(j + 1).min(input.len() - 1)];
            a + (b - a) * frac
        })
        .collect()
}

#[cfg(feature = "voice")]
pub use recorder::Recorder;

#[cfg(feature = "voice")]
mod recorder {
    use anyhow::{bail, Context, Result};
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use std::sync::{Arc, Mutex};

    /// Microphone capture. Not `Send`: keep it on the UI thread.
    pub struct Recorder {
        _stream: cpal::Stream,
        buf: Arc<Mutex<Vec<f32>>>,
        rate: u32,
    }

    impl Recorder {
        pub fn start() -> Result<Self> {
            let host = cpal::default_host();
            let device = host.default_input_device().context("no microphone found")?;
            let config = device.default_input_config()?;
            let rate = config.sample_rate().0;
            let channels = config.channels() as usize;
            let buf = Arc::new(Mutex::new(Vec::<f32>::new()));
            let err = |e| eprintln!("microphone error: {e}");
            macro_rules! build {
                ($t:ty, $conv:expr) => {{
                    let b = buf.clone();
                    device.build_input_stream(
                        &config.clone().into(),
                        move |data: &[$t], _: &cpal::InputCallbackInfo| {
                            let mut v = b.lock().unwrap();
                            for frame in data.chunks(channels) {
                                let sum: f32 = frame.iter().map(|s| $conv(*s)).sum();
                                v.push(sum / channels as f32);
                            }
                            // Keep at most 60 s.
                            let max = rate as usize * 60;
                            if v.len() > max {
                                let cut = v.len() - max;
                                v.drain(..cut);
                            }
                        },
                        err,
                        None,
                    )?
                }};
            }
            let stream = match config.sample_format() {
                cpal::SampleFormat::F32 => build!(f32, |s: f32| s),
                cpal::SampleFormat::I16 => build!(i16, |s: i16| s as f32 / 32768.0),
                cpal::SampleFormat::U16 => build!(u16, |s: u16| (s as f32 - 32768.0) / 32768.0),
                f => bail!("unsupported microphone format {f:?}"),
            };
            stream.play()?;
            Ok(Self {
                _stream: stream,
                buf,
                rate,
            })
        }

        /// Everything recorded so far, as 16 kHz mono; clears the buffer.
        pub fn take(&self) -> Vec<f32> {
            let v = std::mem::take(&mut *self.buf.lock().unwrap());
            super::resample_16k(&v, self.rate)
        }

        /// The most recent `secs` seconds (16 kHz), without clearing.
        pub fn last(&self, secs: f32) -> Vec<f32> {
            let v = self.buf.lock().unwrap();
            let n = ((self.rate as f32 * secs) as usize).min(v.len());
            super::resample_16k(&v[v.len() - n..], self.rate)
        }

        pub fn clear(&self) {
            self.buf.lock().unwrap().clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wake_words() {
        assert!(has_wake_word("Hey, Orbit! What's the weather?"));
        assert_eq!(
            after_wake_word("Hey, Orbit! play some music"),
            "play some music"
        );
        assert!(!has_wake_word("orbital mechanics"));
    }

    #[test]
    fn cleans() {
        assert_eq!(
            clean_transcript(" [BLANK_AUDIO]\n Hello there. (music) "),
            "Hello there."
        );
        assert_eq!(
            speakable("# Title\nSome **bold**\n```run\nls\n```\n- item"),
            "Title Some bold item"
        );
    }

    #[test]
    fn wav_and_resample() {
        let p = std::env::temp_dir().join(format!("orbit-{}.wav", std::process::id()));
        write_wav(&[0.0, 0.5, -0.5], &p).unwrap();
        assert_eq!(std::fs::metadata(&p).unwrap().len(), 44 + 6);
        std::fs::remove_file(p).ok();
        assert_eq!(resample_16k(&vec![0.0; 48_000], 48_000).len(), 16_000);
    }
}
