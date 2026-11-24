//! The OrbitOS desktop application state and UI.

use crate::theme;
use eframe::egui::{self, Color32, RichText};
use orbit_core::{
    apps, claude, exec, files, human_bytes, system, BackendKind, Config, Message, Role,
};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

mod files_ui;
mod icons;
mod media_ui;
mod office_ui;
mod shield;

enum Event {
    ClaudeChunk(String),
    ClaudeDone(Result<String, String>),
    Term(String),
    TermDone,
    ScanProgress(usize, String),
    ScanDone(orbit_core::antivirus::ScanReport),
    Realtime(Vec<orbit_core::antivirus::Finding>),
    ShieldUpdate(Result<String, String>),
    MediaLibrary(Vec<orbit_core::media::Item>, Vec<orbit_core::media::Item>),
    OfficeMsg(Result<String, String>),
    DraftDone(Result<PathBuf, String>),
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
}

impl Desktop {
    pub fn new(cfg: Config, ctx: egui::Context) -> Self {
        let (tx, rx) = channel();
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
        };
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
                    self.chat.push(Message::assistant(text));
                }
                Event::ClaudeDone(Err(e)) => {
                    self.streaming = None;
                    self.chat.push(Message::assistant(format!("Error: {e}")));
                }
                Event::Term(s) => self.term_log.push_str(&s),
                Event::TermDone => self.term_busy = false,
                Event::ScanProgress(n, p) => {
                    self.shield.count = n;
                    self.shield.current = p;
                }
                Event::ScanDone(r) => self.on_scan_done(r),
                Event::Realtime(f) => self.on_realtime(f),
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
            }
        }
        self.shield_tick();
        self.media_tick();
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
        self.win.claude = true;
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

impl eframe::App for Desktop {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pump();
        ctx.request_repaint_after(Duration::from_millis(500));

        self.top_bar(ctx);
        self.dock(ctx);
        if self.show_launcher {
            self.launcher(ctx);
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                theme::paint_wallpaper(ui.painter(), rect, ctx.input(|i| i.time));
                // Watermark in the middle of the desktop.
                ui.painter().text(
                    rect.center() - egui::vec2(0.0, 30.0),
                    egui::Align2::CENTER_CENTER,
                    "OrbitOS",
                    egui::FontId::proportional(60.0),
                    Color32::from_white_alpha(40),
                );
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
                                "Ask Claude anything… ({})",
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
        self.confirm_run(ctx);
        self.toast_ui(ctx);

        // Keyboard shortcuts.
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Space)) {
            self.show_launcher = !self.show_launcher;
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::K)) {
            self.win.claude = true;
        }
    }
}

// ------------------------------------------------------------- panels ----

impl Desktop {
    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("bar")
            .exact_height(30.0)
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    if ui
                        .button(RichText::new("OrbitOS").strong().color(theme::ACCENT))
                        .clicked()
                    {
                        self.show_launcher = !self.show_launcher;
                    }
                    ui.menu_button("System", |ui| {
                        if ui.button("Settings").clicked() {
                            self.win.settings = true;
                            ui.close_menu();
                        }
                        if ui.button("System Monitor").clicked() {
                            self.win.monitor = true;
                            ui.close_menu();
                        }
                        if ui.button("About OrbitOS").clicked() {
                            self.win.about = true;
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("Quit OrbitOS").clicked() {
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(
                                chrono::Local::now().format("%a %d %b  %H:%M").to_string(),
                            )
                            .strong(),
                        );
                        ui.separator();
                        let threats = self
                            .shield
                            .findings
                            .iter()
                            .filter(|f| f.severity >= orbit_core::antivirus::Severity::Medium)
                            .count();
                        let (shield_text, shield_col) = if self.shield.scanning {
                            ("Shield: scanning".to_string(), theme::ACCENT)
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
                            self.win.shield = true;
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
                    let total = 10.0 * 104.0;
                    ui.add_space(((ui.available_width() - total) / 2.0).max(0.0));
                    let item = |ui: &mut egui::Ui, label: &str, open: &mut bool| {
                        let text = if *open {
                            RichText::new(label).strong().color(theme::ACCENT)
                        } else {
                            RichText::new(label)
                        };
                        if ui
                            .add_sized([96.0, 32.0], egui::Button::new(text))
                            .clicked()
                        {
                            *open = !*open;
                        }
                    };
                    let mut launcher = self.show_launcher;
                    item(ui, "Apps", &mut launcher);
                    self.show_launcher = launcher;
                    item(ui, "Files", &mut self.win.files);
                    item(ui, "Terminal", &mut self.win.terminal);
                    item(ui, "Claude", &mut self.win.claude);
                    item(ui, "Shield", &mut self.win.shield);
                    let was_media = self.win.media;
                    item(ui, "Media", &mut self.win.media);
                    if self.win.media && !was_media {
                        self.open_media();
                    }
                    let was_office = self.win.office;
                    item(ui, "Office", &mut self.win.office);
                    if self.win.office && !was_office {
                        self.open_office();
                    }
                    item(ui, "Monitor", &mut self.win.monitor);
                    item(ui, "Settings", &mut self.win.settings);
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
        egui::Window::new("Terminal")
            .open(&mut open)
            .default_size([640.0, 260.0])
            .default_pos([680.0, 500.0])
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
                    ui.label(RichText::new("›").monospace().color(theme::ACCENT));
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

    fn claude_window(&mut self, ctx: &egui::Context) {
        let mut open = self.win.claude;
        egui::Window::new(RichText::new("Claude").color(theme::CLAUDE))
            .id(egui::Id::new("claude_win"))
            .open(&mut open)
            .default_size([640.0, 380.0])
            .default_pos([680.0, 48.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(claude::backend_status(&self.cfg)).color(theme::DIM));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("New chat").clicked() && self.streaming.is_none() {
                            self.chat.clear();
                        }
                        if ui.small_button("Send system info").clicked() {
                            let s = self.snap.summary();
                            self.send_to_claude(format!("Here is my system status. Anything I should know or fix?\n\n{s}"));
                        }
                    });
                });
                ui.separator();
                let input_h = 70.0;
                let mut run: Option<String> = None;
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .auto_shrink([false; 2])
                    .max_height(ui.available_height() - input_h)
                    .show(ui, |ui| {
                        if self.chat.is_empty() {
                            ui.label(RichText::new(
                                "Ask about your files, your system, code, or anything else.\n\
                                 When Claude suggests a command you'll get a Run button, and nothing runs until you confirm.",
                            ).color(theme::DIM));
                        }
                        for m in &self.chat {
                            bubble(ui, m.role, &m.content);
                            if m.role == Role::Assistant {
                                for cmd in claude::extract_commands(&m.content) {
                                    if ui.button(RichText::new(format!("▶ Run: {cmd}")).monospace()).clicked() {
                                        run = Some(cmd);
                                    }
                                }
                            }
                        }
                        if let Some(s) = &self.streaming {
                            bubble(ui, Role::Assistant, if s.is_empty() { "…" } else { s });
                        }
                    });
                if run.is_some() {
                    self.pending_run = run;
                }
                ui.separator();
                ui.horizontal(|ui| {
                    let r = ui.add(
                        egui::TextEdit::multiline(&mut self.chat_input)
                            .hint_text("Message Claude (Enter to send, Shift+Enter for a new line)")
                            .desired_rows(2)
                            .desired_width(ui.available_width() - 70.0),
                    );
                    let enter = r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
                    let send = ui.add_enabled(self.streaming.is_none(), egui::Button::new("Send")).clicked();
                    if (enter || send) && self.streaming.is_none() {
                        let text = std::mem::take(&mut self.chat_input).trim().to_string();
                        self.send_to_claude(text);
                    }
                });
            });
        self.win.claude = open;
    }

    fn monitor_window(&mut self, ctx: &egui::Context) {
        let mut open = self.win.monitor;
        let mut kill: Option<u32> = None;
        egui::Window::new("System Monitor")
            .open(&mut open)
            .default_size([600.0, 300.0])
            .default_pos([24.0, 440.0])
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

    fn settings_window(&mut self, ctx: &egui::Context) {
        let mut open = self.win.settings;
        egui::Window::new("Settings")
            .open(&mut open)
            .default_size([460.0, 380.0])
            .show(ctx, |ui| {
                ui.heading("Claude connection");
                ui.radio_value(
                    &mut self.cfg.backend,
                    BackendKind::Cli,
                    "Claude Code CLI: uses your signed-in `claude` (no key stored)",
                );
                ui.radio_value(
                    &mut self.cfg.backend,
                    BackendKind::Api,
                    "Anthropic API: uses an API key",
                );
                ui.radio_value(
                    &mut self.cfg.backend,
                    BackendKind::Auto,
                    "Auto: API if a key is available, otherwise CLI",
                );
                ui.label(
                    RichText::new(format!("Now using: {}", claude::backend_status(&self.cfg)))
                        .color(theme::ACCENT),
                );
                ui.add_space(6.0);
                egui::Grid::new("settings").num_columns(2).show(ui, |ui| {
                    ui.label("API model");
                    ui.text_edit_singleline(&mut self.cfg.model);
                    ui.end_row();
                    ui.label("Max tokens");
                    ui.add(
                        egui::DragValue::new(&mut self.cfg.max_tokens).clamp_range(256..=64_000),
                    );
                    ui.end_row();
                    ui.label("API key");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.key_edit)
                            .password(true)
                            .hint_text("sk-ant-… (optional)"),
                    );
                    ui.end_row();
                    ui.label("CLI path");
                    let mut p = self.cfg.cli_path.clone().unwrap_or_default();
                    if ui
                        .add(egui::TextEdit::singleline(&mut p).hint_text("auto-detect `claude`"))
                        .changed()
                    {
                        self.cfg.cli_path = if p.trim().is_empty() { None } else { Some(p) };
                    }
                    ui.end_row();
                });
                if std::env::var("ANTHROPIC_API_KEY").is_ok() {
                    ui.label(
                        RichText::new(
                            "ANTHROPIC_API_KEY is set in the environment and takes priority.",
                        )
                        .color(theme::DIM),
                    );
                }
                ui.label(
                    RichText::new(format!(
                        "A saved key is stored in plain text in {}",
                        Config::path().display()
                    ))
                    .small()
                    .color(theme::DIM),
                );
                ui.separator();
                ui.checkbox(&mut self.cfg.fullscreen, "Start OrbitOS fullscreen");
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        self.cfg.api_key = if self.key_edit.trim().is_empty() {
                            None
                        } else {
                            Some(self.key_edit.trim().to_string())
                        };
                        match self.cfg.save() {
                            Ok(()) => self.notify("Settings saved"),
                            Err(e) => self.notify(format!("Save failed: {e}")),
                        }
                    }
                    if ui.button("Test Claude").clicked() {
                        self.send_to_claude(
                            "Reply with one short sentence confirming you're connected to OrbitOS."
                                .into(),
                        );
                    }
                });
            });
        self.win.settings = open;
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
        egui::Window::new("Run this command?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label("Claude suggested:");
                ui.add(
                    egui::Label::new(RichText::new(&cmd).monospace().color(theme::WARN)).wrap(true),
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
        Role::User => ("You", theme::ACCENT),
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
