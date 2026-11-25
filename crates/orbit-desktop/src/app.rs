//! The OrbitOS desktop application state and UI.

use crate::theme;
use eframe::egui::{self, Color32, RichText};
use orbit_core::{apps, claude, exec, files, human_bytes, system, Config, Message, Role};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

mod agent;
mod center;
mod claude_ui;
mod cmdbar;
mod files_ui;
mod icons;
mod media_ui;
mod office_ui;
mod settings_ui;
mod shield;
mod start_menu;
mod store_ui;
mod tray;
mod voice_ui;
mod welcome;
mod wm;

use icons::WinId;

enum Event {
    ClaudeChunk(String),
    ClaudeDone(Result<String, String>),
    Term(String),
    TermDone,
    ScanProgress(usize, String),
    ScanDone(orbit_core::antivirus::ScanReport),
    Realtime(Vec<orbit_core::antivirus::Finding>),
    ShieldUpdate(Result<String, String>),
    Sweep(Box<orbit_core::malware::Sweep>),
    TempCleaned(usize, u64, usize),
    Nebula(egui::ColorImage),
    MediaLibrary(Vec<orbit_core::media::Item>, Vec<orbit_core::media::Item>),
    OfficeMsg(Result<String, String>),
    DraftDone(Result<PathBuf, String>),
    ActionDone(usize, usize, orbit_core::actions::Outcome),
    ImageDone(Result<PathBuf, String>),
    Weather(Result<orbit_core::weather::Weather, String>),
    Transcript(voice_ui::Purpose, Result<String, String>),
    StoreChecked(std::collections::HashSet<&'static str>),
    LocalModels(Result<Vec<String>, String>),
    BackupDone(Result<String, String>, bool),
    Network(Result<Vec<orbit_core::network::Connection>, String>),
    RadioResults(Result<Vec<orbit_core::radio::Station>, String>),
    BeMusicResults(Result<Vec<orbit_core::radio::RemoteTrack>, String>),
    Tray(tray::TrayCmd),
    TrayStatus(bool),
    ScheduledScan(i64),
}

#[derive(Default)]
struct Windows {
    files: bool,
    terminal: bool,
    claude: bool,
    monitor: bool,
    settings: bool,
    about: bool,
    shield: bool,
    media: bool,
    office: bool,
    store: bool,
}

pub struct Desktop {
    ctx: egui::Context,
    cfg: Config,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    win: Windows,
    toast: Option<(String, Instant)>,

    // system
    monitor: system::Monitor,
    snap: system::Snapshot,
    last_sample: Instant,

    // launcher
    show_launcher: bool,
    apps: Vec<apps::App>,
    app_filter: String,

    // files
    cwd: PathBuf,
    entries: Vec<files::Entry>,
    files_err: Option<String>,
    path_edit: String,
    show_hidden: bool,

    // terminal
    term_cwd: PathBuf,
    term_log: String,
    term_input: String,
    term_busy: bool,

    // claude
    chat: Vec<Message>,
    chat_input: String,
    streaming: Option<String>,
    pending_run: Option<String>,

    // settings
    key_edit: String,

    // desktop icons
    icons: Vec<icons::DesktopIcon>,
    selected_icon: Option<usize>,
    files_grid: bool,
    file_selected: Option<PathBuf>,

    // Orbit Shield, Media, Office
    shield: shield::ShieldState,
    media: media_ui::MediaState,
    office: office_ui::OfficeState,

    // OrbitOS 0.3: window manager, command bar, notifications, voice, …
    wm: wm::Wm,
    desk_rect: egui::Rect,
    claude: claude_ui::ClaudeState,
    cmd: cmdbar::CmdBar,
    center: center::CenterState,
    voice: voice_ui::VoiceState,
    store: store_ui::StoreState,
    welcome: welcome::WelcomeState,
    settings: settings_ui::SettingsState,
    settings_tab: settings_ui::Tab,
    wallpaper_tex: Option<egui::TextureHandle>,
    nebula_tex: Option<egui::TextureHandle>,
    /// Frames spent waiting for the monitor size (screen fit and 4K setup).
    display_frames: u32,
    display_done: bool,

    // 0.3.1: tray, background agent, Start menu
    ctl: std::sync::Arc<tray::WinCtl>,
    tray_parts: tray::TrayParts,
    primary: Option<orbit_core::instance::Primary>,
    started: bool,
    quitting: bool,
    hidden_at: Option<Instant>,
    background_start: Option<Instant>,
    tray_known: Option<bool>,
    start: start_menu::StartMenu,
    start_button_clicked: bool,
}

impl Desktop {
    pub fn new(
        cfg: Config,
        ctx: egui::Context,
        primary: Option<orbit_core::instance::Primary>,
    ) -> Self {
        let (tx, rx) = channel();
        let ctl = tray::WinCtl::new(ctx.clone(), tx.clone());
        let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let mut monitor = system::Monitor::new();
        let snap = monitor.snapshot(12);
        let mut d = Self {
            ctx,
            key_edit: cfg.api_key.clone().unwrap_or_default(),
            cfg,
            tx,
            rx,
            win: if std::env::args().any(|a| a == "--show-all") {
                Windows {
                    files: true,
                    shield: true,
                    media: true,
                    ..Default::default()
                }
            } else {
                // --open=files,shield,media,office,claude,terminal,monitor,settings
                let list = std::env::args()
                    .find_map(|a| a.strip_prefix("--open=").map(str::to_string))
                    .unwrap_or_default();
                let has = |n: &str| list.split(',').any(|x| x.trim() == n);
                Windows {
                    files: has("files"),
                    terminal: has("terminal"),
                    claude: has("claude"),
                    monitor: has("monitor"),
                    settings: has("settings"),
                    shield: has("shield"),
                    media: has("media"),
                    office: has("office"),
                    store: has("store"),
                    about: false,
                }
            },
            toast: None,
            monitor,
            snap,
            last_sample: Instant::now(),
            show_launcher: false,
            apps: apps::discover(),
            app_filter: String::new(),
            cwd: home.clone(),
            entries: Vec::new(),
            files_err: None,
            path_edit: String::new(),
            show_hidden: false,
            term_cwd: home,
            term_log: format!(
                "OrbitOS terminal on {} ({}). Type `help`.\n",
                orbit_core::host_os(),
                orbit_core::host_shell()
            ),
            term_input: String::new(),
            term_busy: false,
            chat: Vec::new(),
            chat_input: String::new(),
            streaming: None,
            pending_run: None,
            icons: icons::default_icons(),
            selected_icon: None,
            files_grid: true,
            file_selected: None,
            shield: shield::ShieldState::new(false),
            media: media_ui::MediaState::new(),
            office: office_ui::OfficeState::new(),
            wm: wm::Wm::load(),
            desk_rect: egui::Rect::from_min_size(egui::pos2(0.0, 30.0), egui::vec2(1360.0, 780.0)),
            claude: claude_ui::ClaudeState {
                auto_continue: true,
                ..Default::default()
            },
            cmd: cmdbar::CmdBar::default(),
            center: center::CenterState::default(),
            voice: voice_ui::VoiceState::new(),
            store: store_ui::StoreState::default(),
            welcome: welcome::WelcomeState::default(),
            settings: settings_ui::SettingsState::default(),
            settings_tab: settings_ui::Tab::Claude,
            wallpaper_tex: None,
            nebula_tex: None,
            display_frames: 0,
            display_done: false,
            ctl,
            tray_parts: tray::TrayParts::default(),
            primary,
            started: false,
            quitting: false,
            hidden_at: None,
            background_start: None,
            tray_known: None,
            start: start_menu::StartMenu::default(),
            start_button_clicked: false,
        };
        d.chat = Self::load_current_chat();
        d.welcome.open = !d.cfg.onboarded
            && !std::env::args().any(|a| a.starts_with("--open=") || a == "--show-all");
        d.start_file_index();
        d.refresh_store();
        if d.cfg.wallpaper == "image" {
            d.load_wallpaper_image();
        }
        // Windows asked for on the command line appear on the current desktop.
        for k in wm::MANAGED {
            if *d.win_flag(k) {
                d.wm_show(k);
            }
        }
        // Start-up options: --center, --search=<text>, --ask=<text>
        for arg in std::env::args() {
            if arg == "--center" {
                d.center.open = true;
            } else if let Some(q) = arg.strip_prefix("--search=") {
                d.open_cmdbar();
                d.cmd.query = q.to_string();
            } else if let Some(q) = arg.strip_prefix("--ask=") {
                d.send_to_claude(q.to_string());
            }
        }
        d.refresh_files();
        if d.win.media {
            d.open_media();
        }
        if d.win.office {
            d.open_office();
        }
        d
    }

    fn notify(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    // --------------------------------------------------------- events --

    fn pump(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                Event::ClaudeChunk(c) => {
                    self.streaming.get_or_insert_with(String::new).push_str(&c)
                }
                Event::ClaudeDone(Ok(text)) => {
                    self.streaming = None;
                    if self.cfg.speak_replies {
                        self.speak(&text);
                    }
                    if !self.wm_visible(WinId::Claude) || !self.win.claude {
                        let preview: String = claude_ui::strip_action_blocks(&text)
                            .chars()
                            .take(90)
                            .collect();
                        self.push_note("Claude replied", &preview);
                    }
                    self.chat.push(Message::assistant(text));
                    self.save_chat();
                }
                Event::ClaudeDone(Err(e)) => {
                    self.streaming = None;
                    self.chat.push(Message::assistant(format!("Error: {e}")));
                    self.save_chat();
                }
                Event::Term(s) => self.term_log.push_str(&s),
                Event::TermDone => self.term_busy = false,
                Event::ScanProgress(n, p) => {
                    self.shield.count = n;
                    self.shield.current = p;
                }
                Event::ScanDone(r) => self.on_scan_done(r),
                Event::Realtime(f) => self.on_realtime(f),
                Event::Sweep(s) => self.on_sweep(*s),
                Event::TempCleaned(n, b, k) => self.on_temp_cleaned(n, b, k),
                Event::Nebula(img) => {
                    self.nebula_tex = Some(self.ctx.load_texture(
                        "nebula",
                        img,
                        egui::TextureOptions::LINEAR,
                    ));
                }
                Event::ShieldUpdate(r) => {
                    self.shield.updating = false;
                    self.shield.clamav = orbit_core::antivirus::clamav_version();
                    match r {
                        Ok(m) => self.notify(format!("ClamAV: {m}")),
                        Err(e) => self.notify(format!("Signature update failed: {e}")),
                    }
                }
                Event::MediaLibrary(m, v) => self.on_media_library(m, v),
                Event::OfficeMsg(r) => {
                    let msg = r.unwrap_or_else(|e| e);
                    self.office.status = Some(msg.clone());
                    self.notify(msg);
                    self.refresh_files();
                    self.office.recent = orbit_core::office::recent_documents(25);
                }
                Event::DraftDone(r) => self.on_draft_done(r),
                Event::Tray(cmd) => match cmd {
                    tray::TrayCmd::Search => self.open_cmdbar(),
                    tray::TrayCmd::Ask => self.open_win(WinId::Claude),
                    tray::TrayCmd::Scan => {
                        self.open_win(WinId::Shield);
                        self.start_scan(orbit_core::antivirus::quick_scan_paths());
                    }
                    tray::TrayCmd::Quit => self.quit(),
                },
                Event::TrayStatus(ok) => {
                    if !ok && self.tray_known.is_none() && self.cfg.close_to_tray {
                        self.push_note(
                            "No system tray here",
                            "Closing Orbit will minimise it to the taskbar instead.",
                        );
                    }
                    self.tray_known = Some(ok);
                }
                Event::ScheduledScan(day) => {
                    self.cfg.last_scheduled_scan_day = day;
                    let _ = self.cfg.save();
                    self.shield.scanning = true;
                    self.shield.summary = None;
                    self.push_note("Orbit Shield", "Running your daily quick scan");
                }
                Event::ActionDone(m, c, out) => self.on_action_done(m, c, out),
                Event::ImageDone(r) => {
                    self.claude.generating_image = false;
                    match r {
                        Ok(p) => self.apply_effect(orbit_core::actions::UiEffect::ShowImage(p)),
                        Err(e) => self.push_note("Image generation", &e),
                    }
                }
                Event::Weather(r) => {
                    self.center.weather_loading = false;
                    match r {
                        Ok(w) => {
                            self.center.weather = Some(w);
                            self.center.weather_err = None;
                        }
                        Err(e) => self.center.weather_err = Some(e),
                    }
                }
                Event::Transcript(p, r) => self.on_transcript(p, r),
                Event::StoreChecked(set) => {
                    self.store.installed = set;
                    self.store.checking = false;
                    self.store.checked = true;
                }
                Event::LocalModels(r) => match r {
                    Ok(models) => {
                        self.settings.local_status =
                            Some(format!("Connected: {} model(s)", models.len()));
                        self.settings.local_models = models;
                    }
                    Err(e) => self.settings.local_status = Some(format!("Failed: {e}")),
                },
                Event::BackupDone(r, quiet) => match r {
                    Ok(p) => {
                        self.settings.backup_status = Some(format!("Saved {p}"));
                        if !quiet {
                            self.push_note("Backup complete", &p);
                        }
                    }
                    Err(e) => {
                        self.settings.backup_status = Some(format!("Backup failed: {e}"));
                        self.push_note("Backup failed", &e);
                    }
                },
                Event::Network(r) => self.on_network(r),
                Event::RadioResults(r) => {
                    self.media.radio_loading = false;
                    match r {
                        Ok(v) => self.media.radio_results = v,
                        Err(e) => self.notify(e),
                    }
                }
                Event::BeMusicResults(r) => {
                    self.media.be_loading = false;
                    match r {
                        Ok(v) => {
                            self.media.be_results = v;
                            self.media.be_err = None;
                        }
                        Err(e) => self.media.be_err = Some(e),
                    }
                }
            }
        }
        self.media_tick();
        self.weather_tick();
        self.backup_tick();
        if self.last_sample.elapsed() > Duration::from_millis(1500) {
            self.snap = self.monitor.snapshot(12);
            self.last_sample = Instant::now();
        }
    }

    // --------------------------------------------------------- claude --

    fn send_to_claude(&mut self, text: String) {
        if text.trim().is_empty() || self.streaming.is_some() {
            return;
        }
        if !self.win.claude {
            self.win.claude = true;
            self.wm_show(WinId::Claude);
        }
        self.chat.push(Message::user(text));
        self.streaming = Some(String::new());
        let cfg = self.cfg.clone();
        let history = self.chat.clone();
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        let cwd = self.term_cwd.clone();
        std::thread::spawn(move || {
            let _ = std::env::set_current_dir(&cwd);
            let res = claude::ask(&cfg, &history, &mut |chunk| {
                let _ = tx.send(Event::ClaudeChunk(chunk.to_string()));
                ctx.request_repaint();
            });
            let _ = tx.send(Event::ClaudeDone(res.map_err(|e| e.to_string())));
            ctx.request_repaint();
        });
    }

    fn ask_about_file(&mut self, path: &std::path::Path) {
        match files::read_text_preview(path, 24_000) {
            Some(text) => self.send_to_claude(format!(
                "Explain this file ({}) briefly and point out anything worth fixing:\n\n{text}",
                path.display()
            )),
            None => self.notify("That file isn't readable text."),
        }
    }

    // ------------------------------------------------------- terminal --

    fn run_in_terminal(&mut self, line: String) {
        self.win.terminal = true;
        let line = line.trim().to_string();
        if line.is_empty() {
            return;
        }
        self.term_log
            .push_str(&format!("\n{}> {line}\n", self.term_cwd.display()));
        match line.as_str() {
            "clear" | "cls" => {
                self.term_log.clear();
                return;
            }
            "help" => {
                self.term_log.push_str(
                    "Built-ins: cd <dir>, clear, help, ? <question for Claude>.\nEverything else runs in the host shell.\n",
                );
                return;
            }
            _ => {}
        }
        if let Some(q) = line.strip_prefix("? ") {
            self.send_to_claude(q.to_string());
            self.term_log.push_str("(sent to Claude)\n");
            return;
        }
        if line == "cd" || line.starts_with("cd ") {
            let arg = line[2..].trim();
            let target = if arg.is_empty() {
                std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                    .map(PathBuf::from)
                    .unwrap_or_default()
            } else {
                let p = orbit_core::expand_tilde(arg);
                if p.is_absolute() {
                    p
                } else {
                    self.term_cwd.join(p)
                }
            };
            match target.canonicalize() {
                Ok(p) if p.is_dir() => self.term_cwd = p,
                _ => self.term_log.push_str("cd: no such directory\n"),
            }
            return;
        }
        if self.term_busy {
            self.term_log
                .push_str("(busy: wait for the previous command)\n");
            return;
        }
        self.term_busy = true;
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        let cwd = self.term_cwd.clone();
        std::thread::spawn(move || {
            let msg = match exec::run_capture(&line, &cwd) {
                Ok(out) => {
                    let mut s = out.stdout;
                    s.push_str(&out.stderr);
                    if !out.status.success() {
                        s.push_str(&format!("[exit: {}]\n", out.status));
                    }
                    s
                }
                Err(e) => format!("error: {e}\n"),
            };
            let _ = tx.send(Event::Term(msg));
            let _ = tx.send(Event::TermDone);
            ctx.request_repaint();
        });
    }

    // ---------------------------------------------------------- files --

    fn refresh_files(&mut self) {
        self.path_edit = self.cwd.display().to_string();
        match files::list(&self.cwd, self.show_hidden) {
            Ok(e) => {
                self.entries = e;
                self.files_err = None;
            }
            Err(e) => {
                self.entries.clear();
                self.files_err = Some(e.to_string());
            }
        }
    }

    fn go_to(&mut self, p: PathBuf) {
        if p.is_dir() {
            self.cwd = p;
            self.refresh_files();
        } else {
            self.notify("Not a folder");
        }
    }
}

impl Desktop {
    fn first_frame(&mut self, frame: &eframe::Frame) {
        self.ctl.handle.store(
            tray::window_handle(frame),
            std::sync::atomic::Ordering::Relaxed,
        );
        self.tray_parts = tray::start_tray(&self.ctl);
        if self.cfg.global_hotkey {
            if let Err(e) = self.tray_parts.set_hotkey(&self.ctl, true) {
                self.push_note("Show/hide shortcut unavailable", &e);
            }
        }
        agent::spawn(self.ctl.clone(), self.primary.take());
        if std::env::args().any(|a| a == "--background" || a == "--hide") {
            self.background_start = Some(Instant::now());
        }
    }

    /// Fit the screen: pick the interface size (4K screens get larger text)
    /// and load the HD wallpaper at the screen's real resolution.
    /// Returns true once the scale has taken effect (one frame after it is set).
    fn display_setup(&mut self, ctx: &egui::Context) -> bool {
        if self.display_done {
            if self.display_frames != u32::MAX {
                self.display_frames = u32::MAX;
                return false; // the zoom set last frame applies from this one
            }
            return true;
        }
        self.display_frames += 1;
        let (monitor, native) = ctx.input(|i| {
            (
                i.viewport().monitor_size,
                i.viewport().native_pixels_per_point,
            )
        });
        if monitor.is_none() && self.display_frames < 60 {
            return false;
        }
        self.display_done = true;
        let fullscreen = self.cfg.fullscreen || std::env::args().any(|a| a == "--fullscreen");
        if !fullscreen && self.background_start.is_none() {
            // A remembered window size must not stop Orbit filling the screen.
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
        }
        let ppp = ctx.pixels_per_point();
        let native = native.unwrap_or(ppp);
        let points = monitor.unwrap_or_else(|| ctx.screen_rect().size());
        let physical = points * ppp;
        self.apply_ui_scale_for(physical.x, native);
        // Wallpaper at native resolution (never upscaled beyond the 4K master).
        let (w, h) = (
            physical.x.round().max(640.0) as u32,
            physical.y.round().max(400.0) as u32,
        );
        let (tx, ctx2) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let Ok(img) =
                image::load_from_memory(include_bytes!("../assets/wallpapers/orbit-nebula-4k.jpg"))
            else {
                return;
            };
            let img = if w < img.width() || h < img.height() {
                img.resize_to_fill(
                    w.min(img.width()),
                    h.min(img.height()),
                    image::imageops::FilterType::Lanczos3,
                )
            } else {
                img
            };
            let rgba = img.to_rgba8();
            let ci = egui::ColorImage::from_rgba_unmultiplied(
                [rgba.width() as usize, rgba.height() as usize],
                rgba.as_raw(),
            );
            let _ = tx.send(Event::Nebula(ci));
            ctx2.request_repaint();
        });
        false
    }

    fn apply_ui_scale_for(&mut self, physical_width: f32, native_ppp: f32) {
        let zoom = match self.cfg.ui_scale {
            Some(z) => z.clamp(0.75, 3.0),
            None => {
                let logical = physical_width / native_ppp.max(0.5);
                if logical > 2400.0 {
                    (logical / 1920.0).clamp(1.0, 2.5)
                } else {
                    1.0
                }
            }
        };
        self.ctx.set_zoom_factor(zoom);
    }

    /// Re-apply the interface size after a Settings change.
    pub(super) fn apply_ui_scale(&mut self) {
        let (monitor, native) = self.ctx.input(|i| {
            (
                i.viewport().monitor_size,
                i.viewport().native_pixels_per_point,
            )
        });
        let ppp = self.ctx.pixels_per_point();
        let physical = monitor.unwrap_or_else(|| self.ctx.screen_rect().size()).x * ppp;
        self.apply_ui_scale_for(physical, native.unwrap_or(ppp / self.ctx.zoom_factor()));
    }

    /// Hide to the tray (or minimise when this desktop has no tray).
    pub(super) fn hide_to_tray(&mut self) {
        self.cmd.open = false;
        self.start.open = false;
        self.wm.save();
        self.save_chat();
        if !self.ctl.tray_ok.load(std::sync::atomic::Ordering::Relaxed) {
            self.ctx
                .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            return;
        }
        if !self.cfg.tray_tip_shown
            && self.background_start.is_none()
            && !std::env::args().any(|a| a == "--background")
        {
            self.cfg.tray_tip_shown = true;
            tray::notify_native(
                "Orbit is still running",
                &format!(
                    "Click the Orbit icon in the tray or press {} to bring it back.",
                    tray::hotkey_label()
                ),
            );
        }
        if !self.cfg.listen_while_hidden {
            self.voice.mode = voice_ui::Mode::Idle;
            #[cfg(feature = "voice")]
            {
                self.voice.rec = None;
            }
        }
        let _ = self.cfg.save();
        self.hidden_at = Some(Instant::now());
        self.ctl.hide();
    }

    /// Quit for real: saves everything (in on_exit), then closes.
    pub(super) fn quit(&mut self) {
        self.quitting = true;
        self.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    pub(super) fn set_hotkey(&mut self, on: bool) {
        if let Err(e) = self.tray_parts.set_hotkey(&self.ctl, on) {
            self.push_note("Show/hide shortcut unavailable", &e);
        }
    }
}

impl eframe::App for Desktop {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if !self.started {
            self.started = true;
            self.first_frame(frame);
        }
        if self.ctl.is_hidden() {
            // macOS: the Dock icon can bring a hidden app back by itself.
            #[cfg(target_os = "macos")]
            if self
                .hidden_at
                .map(|t| t.elapsed() > Duration::from_secs(1))
                .unwrap_or(true)
            {
                self.ctl
                    .hidden
                    .store(false, std::sync::atomic::Ordering::Relaxed);
            }
            // Elsewhere a hidden window draws nothing: just collect events.
            #[cfg(not(target_os = "macos"))]
            {
                self.pump();
                return;
            }
        }
        // Closing the window hides Orbit to the tray; only Quit exits.
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting && self.cfg.close_to_tray
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.hide_to_tray();
            return;
        }
        if let Some(t) = self.background_start {
            if self.ctl.tray_ok.load(std::sync::atomic::Ordering::Relaxed) {
                self.background_start = None;
                self.hide_to_tray();
                return;
            } else if t.elapsed() > Duration::from_secs(3) {
                self.background_start = None;
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Q)) {
            self.quit();
        }
        self.start_button_clicked = false;

        self.pump();
        if !self.display_setup(ctx) {
            // Screen size and interface scale are settled first, so windows open at the right size.
            ctx.request_repaint();
            return;
        }
        self.voice_tick(ctx);
        let animated = self.cfg.animate_wallpaper && theme::is_animated(&self.cfg.wallpaper);
        ctx.request_repaint_after(Duration::from_millis(if animated { 50 } else { 500 }));

        // Keyboard shortcuts.
        let (cmd_space, alt_space, cmd_k) = ctx.input(|i| {
            (
                i.modifiers.command && !i.modifiers.alt && i.key_pressed(egui::Key::Space),
                i.modifiers.alt && !i.modifiers.command && i.key_pressed(egui::Key::Space),
                i.modifiers.command && i.key_pressed(egui::Key::K),
            )
        });
        if cmd_space || alt_space {
            if self.cmd.open {
                self.cmd.open = false;
            } else {
                self.open_cmdbar();
            }
        }
        if cmd_k {
            self.open_win(WinId::Claude);
        }
        self.wm_shortcuts(ctx);

        self.top_bar(ctx);
        self.dock(ctx);
        self.center_panel(ctx);
        if self.show_launcher {
            self.launcher(ctx);
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                self.desk_rect = rect;
                let tex = if self.cfg.wallpaper == "nebula" {
                    self.nebula_tex.as_ref()
                } else {
                    self.wallpaper_tex.as_ref()
                };
                theme::paint_wallpaper(ui.painter(), rect, ctx.input(|i| i.time), &self.cfg, tex);
                if self.cfg.wallpaper != "image" {
                    theme::paint_logo(ui.painter(), rect);
                }
                self.desktop_icons(ui, rect);
                // Quick-ask box near the bottom; text kept in egui memory between frames.
                let bar = egui::Rect::from_center_size(
                    egui::pos2(rect.center().x, rect.max.y - 34.0),
                    egui::vec2(480.0, 30.0),
                );
                ui.allocate_ui_at_rect(bar, |ui| {
                    let id = egui::Id::new("quick_ask_text");
                    let mut stored: String = ui.data_mut(|d| d.get_temp(id).unwrap_or_default());
                    let r2 = ui.add(
                        egui::TextEdit::singleline(&mut stored)
                            .hint_text(format!(
                                "Ask Claude anything… ({}) · Ctrl+Space to search",
                                claude::backend_status(&self.cfg)
                            ))
                            .desired_width(480.0),
                    );
                    if r2.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        let q = std::mem::take(&mut stored);
                        self.send_to_claude(q);
                    }
                    ui.data_mut(|d| d.insert_temp(id, stored));
                });
                if self.wm.desktop > 0 {
                    ui.painter().text(
                        rect.right_bottom() - egui::vec2(16.0, 60.0),
                        egui::Align2::RIGHT_BOTTOM,
                        format!("Desktop {}", self.wm.desktop + 1),
                        egui::FontId::proportional(14.0),
                        Color32::from_white_alpha(120),
                    );
                }
            });

        self.files_window(ctx);
        self.terminal_window(ctx);
        self.claude_window(ctx);
        self.monitor_window(ctx);
        self.settings_window(ctx);
        self.about_window(ctx);
        self.shield_window(ctx);
        self.media_window(ctx);
        self.office_window(ctx);
        self.store_window(ctx);
        self.wm_track(ctx);
        self.start_menu_ui(ctx);
        self.welcome_ui(ctx);
        self.confirm_run(ctx);
        self.cmdbar_ui(ctx);
        self.toast_ui(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.wm.save();
        self.save_chat();
        self.media.stream.stop();
        let _ = self.cfg.save();
    }
}

// ------------------------------------------------------------- panels ----

impl Desktop {
    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("bar")
            .exact_height(30.0)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.menu_button(
                        RichText::new("OrbitOS").strong().color(theme::accent()),
                        |ui| {
                            if ui.button("Search  (Ctrl+Space)").clicked() {
                                self.open_cmdbar();
                                ui.close_menu();
                            }
                            if ui.button("Installed apps").clicked() {
                                self.show_launcher = true;
                                ui.close_menu();
                            }
                            if ui.button("App Store").clicked() {
                                self.open_win(WinId::Store);
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui.button("Settings").clicked() {
                                self.open_win(WinId::Settings);
                                ui.close_menu();
                            }
                            if ui.button("System Monitor").clicked() {
                                self.open_win(WinId::Monitor);
                                ui.close_menu();
                            }
                            if ui.button("Welcome tour").clicked() {
                                self.welcome.open = true;
                                self.welcome.step = 0;
                                ui.close_menu();
                            }
                            if ui.button("About OrbitOS").clicked() {
                                self.win.about = true;
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui
                                .button(format!("Hide to tray  ({})", tray::hotkey_label()))
                                .clicked()
                            {
                                ui.close_menu();
                                self.hide_to_tray();
                            }
                            if ui.button("Quit Orbit  (Ctrl+Q)").clicked() {
                                self.quit();
                            }
                        },
                    );
                    if ui
                        .add(
                            egui::Button::new(RichText::new("Search…").color(theme::DIM))
                                .frame(false),
                        )
                        .on_hover_text("Command bar: Ctrl+Space")
                        .clicked()
                    {
                        self.open_cmdbar();
                    }
                    ui.separator();
                    self.mic_button(ui);
                    if !self.voice.status.is_empty() && self.voice.mode != voice_ui::Mode::Idle {
                        ui.label(RichText::new(&self.voice.status).small().color(theme::DIM));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let bell = if self.center.unread > 0 {
                            format!("Alerts {}", self.center.unread)
                        } else {
                            "Alerts".to_string()
                        };
                        let col = if self.center.unread > 0 {
                            theme::WARN
                        } else {
                            ui.visuals().text_color()
                        };
                        if ui
                            .add(egui::Button::new(RichText::new(bell).color(col)).frame(false))
                            .on_hover_text("Notifications and widgets")
                            .clicked()
                        {
                            self.center.open = !self.center.open;
                        }
                        ui.label(
                            RichText::new(
                                chrono::Local::now().format("%a %d %b  %H:%M").to_string(),
                            )
                            .strong(),
                        );
                        ui.separator();
                        self.desktop_switcher(ui);
                        ui.separator();
                        let threats = self
                            .shield
                            .findings
                            .iter()
                            .filter(|f| f.severity >= orbit_core::antivirus::Severity::Medium)
                            .count();
                        let (shield_text, shield_col) = if self.shield.scanning {
                            ("Shield: scanning".to_string(), theme::accent())
                        } else if threats > 0 {
                            (format!("Shield: {threats} threat(s)"), theme::BAD)
                        } else {
                            ("Shield: protected".to_string(), theme::SHIELD_GREEN)
                        };
                        if ui
                            .add(
                                egui::Button::new(RichText::new(shield_text).color(shield_col))
                                    .frame(false),
                            )
                            .clicked()
                        {
                            self.open_win(WinId::Shield);
                        }
                        ui.separator();
                        let busy = self.streaming.is_some();
                        ui.label(
                            RichText::new(if busy {
                                "Claude is thinking…"
                            } else {
                                "Claude ready"
                            })
                            .color(theme::CLAUDE),
                        );
                        ui.separator();
                        let mem = self.snap.mem_fraction();
                        ui.label(
                            RichText::new(format!("RAM {:.0}%", mem * 100.0))
                                .color(theme::load_colour(mem)),
                        );
                        let cpu = self.snap.cpu_total / 100.0;
                        ui.label(
                            RichText::new(format!("CPU {:.0}%", self.snap.cpu_total))
                                .color(theme::load_colour(cpu)),
                        );
                    });
                });
            });
    }

    fn dock(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("dock")
            .exact_height(46.0)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    let total = 10.0 * 100.0;
                    ui.add_space(((ui.available_width() - total) / 2.0).max(0.0));
                    let orbit = RichText::new("Orbit").strong().color(theme::accent());
                    let r = ui
                        .add_sized(
                            [92.0, 32.0],
                            egui::Button::new(orbit)
                                .stroke(egui::Stroke::new(1.5_f32, theme::accent())),
                        )
                        .on_hover_text(
                            "Orbit Start: search, pinned apps, recent documents, Hide and Quit",
                        );
                    if r.clicked() {
                        self.start_button_clicked = true;
                        self.toggle_start();
                    }
                    for k in wm::MANAGED {
                        self.dock_item(ui, k);
                    }
                });
            });
    }

    fn launcher(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("launcher")
            .default_width(280.0)
            .show(ctx, |ui| {
                ui.add_space(6.0);
                ui.heading("Applications");
                ui.add(
                    egui::TextEdit::singleline(&mut self.app_filter)
                        .hint_text("Search apps…")
                        .desired_width(f32::INFINITY),
                );
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{} found", self.apps.len())).color(theme::DIM));
                    if ui.small_button("Rescan").clicked() {
                        self.apps = apps::discover();
                    }
                });
                ui.separator();
                let mut to_launch = None;
                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        for a in apps::find(&self.apps, &self.app_filter) {
                            if ui.add(egui::Button::new(&a.name).frame(false)).clicked() {
                                to_launch = Some(a.clone());
                            }
                        }
                        if self.apps.is_empty() {
                            ui.label(
                                RichText::new("No installed apps were found on this system.")
                                    .color(theme::DIM),
                            );
                        }
                    });
                if let Some(a) = to_launch {
                    match apps::launch(&a) {
                        Ok(()) => self.notify(format!("Launched {}", a.name)),
                        Err(e) => self.notify(format!("Could not launch {}: {e}", a.name)),
                    }
                }
            });
    }

    fn terminal_window(&mut self, ctx: &egui::Context) {
        let mut open = self.win.terminal;
        if !self.wm_visible(WinId::Terminal) {
            return;
        }
        self.wm_window(WinId::Terminal, "Terminal", [680.0, 500.0, 640.0, 260.0])
            .open(&mut open)
            .show(ctx, |ui| {
                let input_h = 34.0;
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .auto_shrink([false; 2])
                    .max_height(ui.available_height() - input_h)
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(RichText::new(&self.term_log).monospace()).wrap(true),
                        );
                    });
                ui.horizontal(|ui| {
                    ui.label(RichText::new("›").monospace().color(theme::accent()));
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut self.term_input)
                            .font(egui::TextStyle::Monospace)
                            .hint_text(if self.term_busy {
                                "running…"
                            } else {
                                "command, or ? question"
                            })
                            .desired_width(f32::INFINITY),
                    );
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        let line = std::mem::take(&mut self.term_input);
                        self.run_in_terminal(line);
                        r.request_focus();
                    }
                });
            });
        self.win.terminal = open;
    }

    fn monitor_window(&mut self, ctx: &egui::Context) {
        let mut open = self.win.monitor;
        let mut kill: Option<u32> = None;
        if !self.wm_visible(WinId::Monitor) {
            return;
        }
        self.wm_window(
            WinId::Monitor,
            "System Monitor",
            [224.0, 440.0, 600.0, 300.0],
        )
        .open(&mut open)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    let s = &self.snap;
                    ui.label(RichText::new(format!("{} · {}", s.os, s.host)).strong());
                    ui.label(
                        RichText::new(format!(
                            "Kernel {} · up {} · {} processes",
                            s.kernel,
                            s.uptime_text(),
                            s.proc_count
                        ))
                        .color(theme::DIM),
                    );
                    ui.label(RichText::new(&s.cpu_brand).color(theme::DIM));
                    ui.add_space(4.0);
                    meter(
                        ui,
                        "CPU",
                        s.cpu_total / 100.0,
                        format!("{:.0}%", s.cpu_total),
                    );
                    meter(
                        ui,
                        "Memory",
                        s.mem_fraction(),
                        format!("{} / {}", human_bytes(s.mem_used), human_bytes(s.mem_total)),
                    );
                    if s.swap_total > 0 {
                        meter(
                            ui,
                            "Swap",
                            s.swap_used as f32 / s.swap_total as f32,
                            format!(
                                "{} / {}",
                                human_bytes(s.swap_used),
                                human_bytes(s.swap_total)
                            ),
                        );
                    }
                    ui.collapsing(format!("Cores ({})", s.cpu_cores.len()), |ui| {
                        egui::Grid::new("cores").num_columns(4).show(ui, |ui| {
                            for (i, c) in s.cpu_cores.iter().enumerate() {
                                ui.add(
                                    egui::ProgressBar::new(c / 100.0)
                                        .desired_width(100.0)
                                        .text(format!("{i}: {c:.0}%"))
                                        .fill(theme::load_colour(c / 100.0)),
                                );
                                if i % 4 == 3 {
                                    ui.end_row();
                                }
                            }
                        });
                    });
                    ui.collapsing("Disks", |ui| {
                        for d in &s.disks {
                            let used = d.total.saturating_sub(d.available);
                            let f = if d.total == 0 {
                                0.0
                            } else {
                                used as f32 / d.total as f32
                            };
                            meter(
                                ui,
                                &d.mount,
                                f,
                                format!(
                                    "{} free of {}",
                                    human_bytes(d.available),
                                    human_bytes(d.total)
                                ),
                            );
                        }
                    });
                    ui.separator();
                    ui.label(RichText::new("Top processes").strong());
                    egui::Grid::new("procs")
                        .striped(true)
                        .num_columns(5)
                        .show(ui, |ui| {
                            for h in ["PID", "Name", "CPU", "Memory", ""] {
                                ui.label(RichText::new(h).color(theme::DIM));
                            }
                            ui.end_row();
                            for p in &s.top_procs {
                                ui.label(p.pid.to_string());
                                ui.label(&p.name);
                                ui.label(format!("{:.1}%", p.cpu));
                                ui.label(human_bytes(p.memory));
                                if ui.small_button("End").clicked() {
                                    kill = Some(p.pid);
                                }
                                ui.end_row();
                            }
                        });
                });
        });
        self.win.monitor = open;
        if let Some(pid) = kill {
            self.pending_run = None;
            let ok = self.monitor.kill(pid);
            self.notify(if ok {
                format!("Sent end signal to {pid}")
            } else {
                format!("Could not end {pid}")
            });
        }
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        let mut open = self.win.about;
        egui::Window::new("About OrbitOS")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(RichText::new("OrbitOS").size(26.0).strong());
                ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                ui.label(format!(
                    "Running on {} · written in Rust",
                    orbit_core::host_os()
                ));
                ui.label("A desktop environment with Claude built in.");
                ui.label(
                    RichText::new("Shortcuts: Ctrl/Cmd+Space apps · Ctrl/Cmd+K Claude")
                        .color(theme::DIM),
                );
            });
        self.win.about = open;
    }

    fn confirm_run(&mut self, ctx: &egui::Context) {
        let Some(cmd) = self.pending_run.clone() else {
            return;
        };
        let mut decision: Option<bool> = None;
        egui::Area::new(egui::Id::new("confirm_run"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                egui::Frame::window(ui.style())
                    .inner_margin(egui::Margin::same(14.0))
                    .show(ui, |ui| {
                        ui.set_max_width(520.0);
                        ui.label(RichText::new("Run this command?").strong().size(16.0));
                        ui.label("OrbitOS will run:");
                        ui.add(
                            egui::Label::new(RichText::new(&cmd).monospace().color(theme::WARN))
                                .wrap(true),
                        );
                        ui.label(
                            RichText::new(format!(
                                "It will run in {} via {}.",
                                self.term_cwd.display(),
                                orbit_core::host_shell()
                            ))
                            .color(theme::DIM),
                        );
                        ui.horizontal(|ui| {
                            if ui.button(RichText::new("Run").strong()).clicked() {
                                decision = Some(true);
                            }
                            if ui.button("Cancel").clicked() {
                                decision = Some(false);
                            }
                        });
                    })
            });
        match decision {
            Some(true) => {
                self.pending_run = None;
                self.run_in_terminal(cmd);
            }
            Some(false) => self.pending_run = None,
            None => {}
        }
    }

    fn toast_ui(&mut self, ctx: &egui::Context) {
        if let Some((msg, at)) = &self.toast {
            if at.elapsed() > Duration::from_secs(3) {
                self.toast = None;
                return;
            }
            egui::Area::new(egui::Id::new("toast"))
                .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -60.0])
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.label(msg.as_str());
                    });
                });
        }
    }
}

fn bubble(ui: &mut egui::Ui, role: Role, text: &str) {
    let (who, col) = match role {
        Role::User => ("You", theme::accent()),
        Role::Assistant => ("Claude", theme::CLAUDE),
    };
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(who).strong().color(col));
        ui.add(egui::Label::new(text).wrap(true).selectable(true));
    });
}

fn meter(ui: &mut egui::Ui, label: &str, frac: f32, text: String) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [70.0, 18.0],
            egui::Label::new(RichText::new(label).color(theme::DIM)).truncate(true),
        );
        ui.add(
            egui::ProgressBar::new(frac.clamp(0.0, 1.0))
                .text(text)
                .fill(theme::load_colour(frac)),
        );
    });
}
