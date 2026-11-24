//! Command bar (Ctrl/Cmd+Space): search apps, files, music, settings and
//! commands, do quick maths, or ask Claude, all from one box.

use super::icons::WinId;
use super::*;
use orbit_core::{calc, search};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub(super) enum CmdAction {
    Copy(String),
    Window(WinId),
    App(orbit_core::apps::App),
    Open(PathBuf),
    Play(PathBuf),
    Radio(String, String),
    QuickScan,
    NewDoc(orbit_core::office::DocKind),
    Theme(&'static str),
    Wallpaper(&'static str),
    Desktop(u8),
    NewChat,
    Welcome,
    Ask(String),
}

#[derive(Clone)]
pub(super) struct CmdResult {
    pub kind: &'static str,
    pub label: String,
    pub detail: String,
    pub action: CmdAction,
}

#[derive(Default)]
pub(super) struct CmdBar {
    pub open: bool,
    pub query: String,
    pub selected: usize,
    pub results: Vec<CmdResult>,
    pub last_query: String,
    pub index: Arc<Mutex<search::FileIndex>>,
    pub indexing: bool,
    pub focus_next: bool,
}

/// Built-in commands people can type.
fn commands() -> Vec<(String, CmdAction)> {
    let mut v: Vec<(String, CmdAction)> = vec![
        ("Quick scan with Orbit Shield".into(), CmdAction::QuickScan),
        ("New chat with Claude".into(), CmdAction::NewChat),
        ("Show welcome tour".into(), CmdAction::Welcome),
    ];
    for w in super::wm::MANAGED {
        v.push((format!("Open {}", w.label()), CmdAction::Window(w)));
    }
    for k in orbit_core::office::DocKind::ALL {
        v.push((
            format!("New {}", k.label().to_lowercase()),
            CmdAction::NewDoc(k),
        ));
    }
    for t in theme::THEMES {
        v.push((format!("Theme: {}", t.name), CmdAction::Theme(t.key)));
    }
    for (k, n) in theme::WALLPAPERS {
        v.push((format!("Wallpaper: {n}"), CmdAction::Wallpaper(k)));
    }
    for d in 0..4u8 {
        v.push((format!("Go to desktop {}", d + 1), CmdAction::Desktop(d)));
    }
    v
}

impl Desktop {
    pub(super) fn start_file_index(&mut self) {
        if self.cmd.indexing {
            return;
        }
        self.cmd.indexing = true;
        let idx = self.cmd.index.clone();
        std::thread::spawn(move || {
            let built = search::FileIndex::home();
            *idx.lock().unwrap() = built;
        });
    }

    pub(super) fn open_cmdbar(&mut self) {
        self.cmd.open = true;
        self.cmd.query.clear();
        self.cmd.last_query = "\u{0}".into();
        self.cmd.selected = 0;
        self.cmd.focus_next = true;
    }

    fn refresh_results(&mut self) {
        let q = self.cmd.query.trim().to_string();
        let ql = q.to_lowercase();
        let mut r: Vec<CmdResult> = Vec::new();
        if q.is_empty() {
            for (label, action) in commands().into_iter().take(8) {
                r.push(CmdResult {
                    kind: "Command",
                    label,
                    detail: String::new(),
                    action,
                });
            }
            self.cmd.results = r;
            return;
        }
        if calc::looks_like_math(&q) {
            if let Some(v) = calc::eval(&q) {
                let s = calc::format(v);
                r.push(CmdResult {
                    kind: "Maths",
                    label: format!("= {s}"),
                    detail: "Enter copies the answer".into(),
                    action: CmdAction::Copy(s),
                });
            }
        }
        for (label, action) in commands() {
            if ql
                .split_whitespace()
                .all(|w| label.to_lowercase().contains(w))
            {
                r.push(CmdResult {
                    kind: "Command",
                    label,
                    detail: String::new(),
                    action,
                });
            }
        }
        for a in orbit_core::apps::find(&self.apps, &q).into_iter().take(6) {
            r.push(CmdResult {
                kind: "App",
                label: a.name.clone(),
                detail: String::new(),
                action: CmdAction::App(a.clone()),
            });
        }
        let tracks: Vec<CmdResult> = self
            .media
            .music
            .iter()
            .filter(|t| {
                t.title.to_lowercase().contains(&ql) || t.artist.to_lowercase().contains(&ql)
            })
            .take(5)
            .map(|t| CmdResult {
                kind: "Music",
                label: t.title.clone(),
                detail: if t.artist.is_empty() {
                    t.folder.clone()
                } else {
                    t.artist.clone()
                },
                action: CmdAction::Play(t.path.clone()),
            })
            .collect();
        r.extend(tracks);
        for s in orbit_core::radio::featured()
            .into_iter()
            .filter(|s| s.name.to_lowercase().contains(&ql))
            .take(3)
        {
            r.push(CmdResult {
                kind: "Radio",
                label: s.name.clone(),
                detail: s.info.clone(),
                action: CmdAction::Radio(s.url, s.name),
            });
        }
        if let Ok(idx) = self.cmd.index.try_lock() {
            for h in idx.query(&q, 8) {
                r.push(CmdResult {
                    kind: if h.is_dir { "Folder" } else { "File" },
                    label: h.name.clone(),
                    detail: search::pretty(&h.path),
                    action: CmdAction::Open(h.path),
                });
            }
        }
        r.push(CmdResult {
            kind: "Claude",
            label: format!("Ask Claude: {q}"),
            detail: claude::backend_status(&self.cfg),
            action: CmdAction::Ask(q),
        });
        self.cmd.results = r;
        self.cmd.selected = 0;
    }

    fn run_cmd(&mut self, a: CmdAction) {
        self.cmd.open = false;
        match a {
            CmdAction::Copy(s) => {
                self.ctx.output_mut(|o| o.copied_text = s.clone());
                self.notify(format!("Copied {s}"));
            }
            CmdAction::Window(w) => self.open_win(w),
            CmdAction::App(app) => match orbit_core::apps::launch(&app) {
                Ok(()) => self.notify(format!("Launched {}", app.name)),
                Err(e) => self.notify(format!("Could not launch: {e}")),
            },
            CmdAction::Open(p) => {
                if p.is_dir() {
                    self.open_win(WinId::Files);
                    self.go_to(p);
                } else if orbit_core::media::kind_of(&p).is_some() {
                    self.play_path(p);
                } else if let Err(e) = exec::open(&p.to_string_lossy()) {
                    self.notify(format!("Could not open: {e}"));
                }
            }
            CmdAction::Play(p) => self.play_path(p),
            CmdAction::Radio(url, name) => self.play_stream(&url, &name, None),
            CmdAction::QuickScan => {
                self.open_win(WinId::Shield);
                self.start_scan(orbit_core::antivirus::quick_scan_paths());
            }
            CmdAction::NewDoc(k) => {
                if let Err(e) = orbit_core::office::new_document(k) {
                    self.notify(e.to_string());
                }
            }
            CmdAction::Theme(t) => {
                self.apply_effect(orbit_core::actions::UiEffect::SetTheme(t.into()))
            }
            CmdAction::Wallpaper(w) => {
                self.apply_effect(orbit_core::actions::UiEffect::SetWallpaper(w.into()))
            }
            CmdAction::Desktop(d) => self.wm.desktop = d,
            CmdAction::NewChat => {
                self.new_chat();
                self.open_win(WinId::Claude);
            }
            CmdAction::Welcome => self.welcome.open = true,
            CmdAction::Ask(q) => {
                self.open_win(WinId::Claude);
                self.send_to_claude(q);
            }
        }
    }

    pub(super) fn cmdbar_ui(&mut self, ctx: &egui::Context) {
        if !self.cmd.open {
            return;
        }
        if self.cmd.query != self.cmd.last_query {
            self.cmd.last_query = self.cmd.query.clone();
            self.refresh_results();
        }
        let (up, down, enter, esc) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::Escape),
            )
        });
        let n = self.cmd.results.len();
        if down && n > 0 {
            self.cmd.selected = (self.cmd.selected + 1) % n;
        }
        if up && n > 0 {
            self.cmd.selected = (self.cmd.selected + n - 1) % n;
        }
        if esc {
            self.cmd.open = false;
            return;
        }
        let mut chosen: Option<CmdAction> = None;
        if enter {
            chosen = self
                .cmd
                .results
                .get(self.cmd.selected)
                .map(|r| r.action.clone());
        }
        // Dim the desktop behind.
        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new("cmd_dim"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let r = ui.allocate_rect(screen, egui::Sense::click());
                ui.painter()
                    .rect_filled(screen, 0.0, Color32::from_black_alpha(110));
                if r.clicked() {
                    self.cmd.open = false;
                }
            });
        egui::Area::new(egui::Id::new("cmd_bar"))
            .order(egui::Order::Tooltip)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 110.0])
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .rounding(egui::Rounding::same(14.0))
                    .inner_margin(egui::Margin::same(12.0))
                    .show(ui, |ui| {
                        ui.set_width(660.0);
                        let r = ui.add(
                            egui::TextEdit::singleline(&mut self.cmd.query)
                                .hint_text(
                                    "Search apps, files, music, settings… do maths… or ask Claude",
                                )
                                .font(egui::FontId::proportional(20.0))
                                .desired_width(f32::INFINITY)
                                .frame(false),
                        );
                        if self.cmd.focus_next {
                            r.request_focus();
                            self.cmd.focus_next = false;
                        }
                        ui.separator();
                        egui::ScrollArea::vertical()
                            .max_height(380.0)
                            .show(ui, |ui| {
                                for (i, res) in self.cmd.results.iter().enumerate() {
                                    let selected = i == self.cmd.selected;
                                    let resp = ui.add_sized(
                                        [ui.available_width(), 34.0],
                                        egui::SelectableLabel::new(selected, ""),
                                    );
                                    let rect = resp.rect;
                                    let p = ui.painter();
                                    p.text(
                                        rect.left_center() + egui::vec2(10.0, 0.0),
                                        egui::Align2::LEFT_CENTER,
                                        res.kind,
                                        egui::FontId::proportional(11.5),
                                        theme::DIM,
                                    );
                                    p.text(
                                        rect.left_center() + egui::vec2(82.0, -1.0),
                                        egui::Align2::LEFT_CENTER,
                                        &res.label,
                                        egui::FontId::proportional(15.0),
                                        ui.visuals().text_color(),
                                    );
                                    if !res.detail.is_empty() {
                                        let d: String = res.detail.chars().take(60).collect();
                                        p.text(
                                            rect.right_center() - egui::vec2(10.0, 0.0),
                                            egui::Align2::RIGHT_CENTER,
                                            d,
                                            egui::FontId::proportional(11.5),
                                            theme::DIM,
                                        );
                                    }
                                    if selected {
                                        resp.scroll_to_me(None);
                                    }
                                    if resp.clicked() {
                                        chosen = Some(res.action.clone());
                                    }
                                }
                            });
                        let idx_len = self.cmd.index.try_lock().map(|i| i.len()).unwrap_or(0);
                        ui.label(
                            RichText::new(format!(
                                "Up/Down to choose · Enter to open · Esc to close · {}",
                                if idx_len == 0 {
                                    "indexing your files…".to_string()
                                } else {
                                    format!("{idx_len} files indexed")
                                }
                            ))
                            .small()
                            .color(theme::DIM),
                        );
                    });
            });
        if let Some(a) = chosen {
            self.run_cmd(a);
        }
    }
}
