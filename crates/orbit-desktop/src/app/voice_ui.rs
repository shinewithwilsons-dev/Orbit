//! Voice control: push-to-talk (mic button or hold F9) and "Hey Orbit".
//! Speech is turned into text locally by whisper.cpp, then sent to Claude.

use super::*;
use orbit_core::voice;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Idle,
    /// Recording while the mic button / F9 is active.
    PushToTalk,
    /// Listening for "Hey Orbit".
    Wake,
    /// Heard the wake word; recording the request.
    Command,
}

pub(super) struct VoiceState {
    pub mode: Mode,
    pub busy: bool,
    pub since: Instant,
    pub last_check: Instant,
    pub last_loud: Instant,
    pub status: String,
    pub speaker: Option<std::process::Child>,
    #[cfg(feature = "voice")]
    pub rec: Option<voice::Recorder>,
}

impl VoiceState {
    pub fn new() -> Self {
        Self {
            mode: Mode::Idle,
            busy: false,
            since: Instant::now(),
            last_check: Instant::now(),
            last_loud: Instant::now(),
            status: String::new(),
            speaker: None,
            #[cfg(feature = "voice")]
            rec: None,
        }
    }
}

/// What a finished transcription was for.
#[derive(Debug, Clone, Copy)]
pub(super) enum Purpose {
    Request,
    WakeCheck,
}

impl Desktop {
    pub(super) fn voice_ready(&self) -> Result<(), String> {
        if !cfg!(feature = "voice") {
            return Err(
                "this build has no microphone support (built without the `voice` feature)".into(),
            );
        }
        if voice::find_whisper(&self.cfg).is_none() || voice::find_model(&self.cfg).is_none() {
            return Err(voice::setup_hint());
        }
        Ok(())
    }

    #[cfg(feature = "voice")]
    fn ensure_recorder(&mut self) -> bool {
        if self.voice.rec.is_none() {
            match voice::Recorder::start() {
                Ok(r) => self.voice.rec = Some(r),
                Err(e) => {
                    self.push_note("Microphone", &e.to_string());
                    return false;
                }
            }
        }
        true
    }

    pub(super) fn ptt_start(&mut self) {
        if let Err(e) = self.voice_ready() {
            self.push_note("Voice isn't set up yet", &e);
            return;
        }
        #[cfg(feature = "voice")]
        {
            if !self.ensure_recorder() {
                return;
            }
            if let Some(r) = &self.voice.rec {
                r.clear();
            }
            self.stop_speaking();
            self.voice.mode = Mode::PushToTalk;
            self.voice.since = Instant::now();
            self.voice.status = "Listening… (click again or release F9 to send)".into();
        }
    }

    pub(super) fn ptt_stop(&mut self) {
        #[cfg(feature = "voice")]
        if self.voice.mode == Mode::PushToTalk {
            let samples = self
                .voice
                .rec
                .as_ref()
                .map(|r| r.take())
                .unwrap_or_default();
            self.voice.mode = if self.cfg.wake_word {
                Mode::Wake
            } else {
                Mode::Idle
            };
            if !self.cfg.wake_word {
                self.voice.rec = None;
            }
            self.transcribe(samples, Purpose::Request);
        }
    }

    fn transcribe(&mut self, samples: Vec<f32>, purpose: Purpose) {
        if samples.len() < 16_000 / 3 {
            if matches!(purpose, Purpose::Request) {
                self.voice.status = "Didn't catch that: hold to talk a little longer".into();
            }
            return;
        }
        self.voice.busy = true;
        if matches!(purpose, Purpose::Request) {
            self.voice.status = "Transcribing…".into();
        }
        let cfg = self.cfg.clone();
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let wav =
                std::env::temp_dir().join(format!("orbit-voice-{}.wav", orbit_core::now_secs()));
            let r = voice::write_wav(&samples, &wav)
                .and_then(|_| voice::transcribe(&cfg, &wav))
                .map_err(|e| e.to_string());
            let _ = std::fs::remove_file(&wav);
            let _ = tx.send(Event::Transcript(purpose, r));
            ctx.request_repaint();
        });
    }

    pub(super) fn on_transcript(&mut self, purpose: Purpose, r: Result<String, String>) {
        self.voice.busy = false;
        match (purpose, r) {
            (Purpose::Request, Ok(text)) => {
                let text = voice::after_wake_word(&text);
                let text = if text.is_empty() { String::new() } else { text };
                if text.trim().is_empty() {
                    self.voice.status = "Didn't catch that".into();
                } else {
                    self.voice.status = format!("You said: {text}");
                    self.send_to_claude(text);
                }
            }
            (Purpose::Request, Err(e)) => {
                self.voice.status.clear();
                self.push_note("Voice", &e);
            }
            (Purpose::WakeCheck, Ok(text)) => {
                if voice::has_wake_word(&text) {
                    let rest = voice::after_wake_word(&text);
                    if rest.split_whitespace().count() >= 2 {
                        // "Hey Orbit, open my documents" said in one go.
                        self.voice.status = format!("You said: {rest}");
                        self.send_to_claude(rest);
                    } else {
                        #[cfg(feature = "voice")]
                        if let Some(r) = &self.voice.rec {
                            r.clear();
                        }
                        self.stop_speaking();
                        self.voice.mode = Mode::Command;
                        self.voice.since = Instant::now();
                        self.voice.last_loud = Instant::now();
                        self.voice.status = "Yes? I'm listening…".into();
                    }
                }
            }
            (Purpose::WakeCheck, Err(_)) => {}
        }
    }

    /// Called every frame: wake-word listening, end-of-speech detection, F9.
    pub(super) fn voice_tick(&mut self, ctx: &egui::Context) {
        // F9 push-to-talk.
        let (f9_down, f9_pressed) =
            ctx.input(|i| (i.key_down(egui::Key::F9), i.key_pressed(egui::Key::F9)));
        if f9_pressed && self.voice.mode != Mode::PushToTalk {
            self.ptt_start();
        } else if !f9_down
            && self.voice.mode == Mode::PushToTalk
            && self.voice.since.elapsed() > Duration::from_millis(300)
            && ctx.input(|i| i.key_released(egui::Key::F9))
        {
            self.ptt_stop();
        }

        #[cfg(feature = "voice")]
        {
            // Start or stop always-on wake-word listening to match the setting.
            if self.cfg.wake_word && self.voice.mode == Mode::Idle && self.voice_ready().is_ok() {
                if self.ensure_recorder() {
                    self.voice.mode = Mode::Wake;
                    self.voice.status = "Say \"Hey Orbit\"".into();
                }
            } else if !self.cfg.wake_word && self.voice.mode == Mode::Wake {
                self.voice.mode = Mode::Idle;
                self.voice.rec = None;
                self.voice.status.clear();
            }
            match self.voice.mode {
                Mode::Wake => {
                    if !self.voice.busy
                        && self.voice.last_check.elapsed() > Duration::from_millis(2000)
                    {
                        self.voice.last_check = Instant::now();
                        let chunk = self
                            .voice
                            .rec
                            .as_ref()
                            .map(|r| r.last(2.5))
                            .unwrap_or_default();
                        if voice::rms(&chunk) > 0.015 {
                            self.transcribe(chunk, Purpose::WakeCheck);
                        }
                    }
                }
                Mode::Command => {
                    let recent = self
                        .voice
                        .rec
                        .as_ref()
                        .map(|r| r.last(0.4))
                        .unwrap_or_default();
                    if voice::rms(&recent) > 0.02 {
                        self.voice.last_loud = Instant::now();
                    }
                    let quiet = self.voice.last_loud.elapsed() > Duration::from_millis(1300);
                    let long = self.voice.since.elapsed() > Duration::from_secs(10);
                    if (quiet && self.voice.since.elapsed() > Duration::from_millis(1500)) || long {
                        let samples = self
                            .voice
                            .rec
                            .as_ref()
                            .map(|r| r.take())
                            .unwrap_or_default();
                        self.voice.mode = Mode::Wake;
                        self.transcribe(samples, Purpose::Request);
                    }
                }
                _ => {}
            }
            if self.voice.mode != Mode::Idle {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }
    }

    pub(super) fn speak(&mut self, text: &str) {
        self.stop_speaking();
        match voice::speak(text) {
            Ok(child) => self.voice.speaker = Some(child),
            Err(e) => self.push_note("Speech", &e.to_string()),
        }
    }

    pub(super) fn stop_speaking(&mut self) {
        if let Some(mut c) = self.voice.speaker.take() {
            let _ = c.kill();
        }
    }

    /// Mic button for the top bar.
    pub(super) fn mic_button(&mut self, ui: &mut egui::Ui) {
        let (text, col) = match self.voice.mode {
            Mode::PushToTalk | Mode::Command => ("● Listening", theme::BAD),
            Mode::Wake => ("Mic: Hey Orbit", theme::OK),
            Mode::Idle if self.voice.busy => ("Mic…", theme::WARN),
            Mode::Idle => ("Mic", theme::DIM),
        };
        let hover = if self.voice.status.is_empty() {
            "Click to talk to Claude (or hold F9). Right-click for voice options".to_string()
        } else {
            self.voice.status.clone()
        };
        let r = ui
            .add(egui::Button::new(RichText::new(text).color(col)).frame(false))
            .on_hover_text(hover);
        if r.clicked() {
            if self.voice.mode == Mode::PushToTalk {
                self.ptt_stop();
            } else {
                self.ptt_start();
            }
        }
        r.context_menu(|ui| {
            if ui
                .checkbox(&mut self.cfg.wake_word, "Listen for \"Hey Orbit\"")
                .changed()
            {
                let _ = self.cfg.save();
            }
            if ui
                .checkbox(&mut self.cfg.speak_replies, "Speak Claude's replies")
                .changed()
            {
                let _ = self.cfg.save();
            }
            if self.voice.speaker.is_some() && ui.button("Stop speaking").clicked() {
                self.stop_speaking();
                ui.close_menu();
            }
            if ui.button("Voice settings…").clicked() {
                self.settings_tab = super::settings_ui::Tab::Voice;
                self.open_win(super::icons::WinId::Settings);
                ui.close_menu();
            }
        });
    }
}
