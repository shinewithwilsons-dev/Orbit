//! Claude window: chat, Run buttons, action cards with Approve/Deny, chat
//! history, /image generation, and spoken replies.

use super::icons::WinId;
use super::*;
use orbit_core::actions::{self, Action, Outcome, Risk, UiEffect};
use std::collections::HashMap;

#[derive(Clone)]
pub(super) enum CardStatus {
    Pending,
    Running,
    Done(Outcome),
    Denied,
}

#[derive(Clone)]
pub(super) struct Card {
    pub action: Result<Action, String>,
    pub status: CardStatus,
}

#[derive(Default)]
pub(super) struct ClaudeState {
    /// Action cards per assistant message index.
    pub cards: HashMap<usize, Vec<Card>>,
    pub auto_continue: bool,
    pub history: Vec<(PathBuf, String)>,
    pub generating_image: bool,
}

fn chats_dir() -> PathBuf {
    let d = Config::data_dir().join("chats");
    let _ = std::fs::create_dir_all(&d);
    d
}

fn risk_colour(r: Risk) -> Color32 {
    match r {
        Risk::Safe => theme::OK,
        Risk::Changes => theme::WARN,
        Risk::Command => theme::BAD,
    }
}

impl Desktop {
    // ---------------------------------------------------------- history --

    pub(super) fn save_chat(&self) {
        if let Ok(s) = serde_json::to_string_pretty(&self.chat) {
            let _ = std::fs::write(chats_dir().join("current.json"), s);
        }
    }

    pub(super) fn load_current_chat() -> Vec<Message> {
        std::fs::read_to_string(chats_dir().join("current.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn refresh_history(&mut self) {
        let mut v: Vec<(PathBuf, String)> = std::fs::read_dir(chats_dir())
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        p.file_stem().map(|s| s != "current").unwrap_or(false)
                            && p.extension().map(|e| e == "json").unwrap_or(false)
                    })
                    .filter_map(|p| {
                        let msgs: Vec<Message> =
                            serde_json::from_str(&std::fs::read_to_string(&p).ok()?).ok()?;
                        let title: String = msgs
                            .iter()
                            .find(|m| m.role == Role::User)?
                            .content
                            .chars()
                            .take(48)
                            .collect();
                        Some((p, title))
                    })
                    .collect()
            })
            .unwrap_or_default();
        v.sort_by(|a, b| b.0.cmp(&a.0));
        v.truncate(20);
        self.claude.history = v;
    }

    pub(super) fn new_chat(&mut self) {
        if self.streaming.is_some() {
            return;
        }
        if !self.chat.is_empty() {
            if let Ok(s) = serde_json::to_string_pretty(&self.chat) {
                let _ = std::fs::write(
                    chats_dir().join(format!("{}.json", orbit_core::now_secs())),
                    s,
                );
            }
        }
        self.chat.clear();
        self.claude.cards.clear();
        self.save_chat();
    }

    fn load_chat(&mut self, path: &std::path::Path) {
        self.new_chat();
        if let Some(msgs) = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<Vec<Message>>(&s).ok())
        {
            self.chat = msgs;
            let _ = std::fs::remove_file(path);
            self.save_chat();
        }
    }

    // ---------------------------------------------------------- actions --

    fn cards_for(&mut self, idx: usize) -> &mut Vec<Card> {
        if !self.claude.cards.contains_key(&idx) {
            let cards = self
                .chat
                .get(idx)
                .filter(|m| m.role == Role::Assistant)
                .map(|m| {
                    actions::extract(&m.content)
                        .into_iter()
                        .map(|a| Card {
                            action: a,
                            status: CardStatus::Pending,
                        })
                        .collect()
                })
                .unwrap_or_default();
            self.claude.cards.insert(idx, cards);
        }
        self.claude.cards.get_mut(&idx).unwrap()
    }

    fn approve(&mut self, msg: usize, card: usize) {
        let Some(Card {
            action: Ok(action),
            status,
        }) = self.cards_for(msg).get_mut(card)
        else {
            return;
        };
        if !matches!(status, CardStatus::Pending) {
            return;
        }
        *status = CardStatus::Running;
        let action = action.clone();
        let cfg = self.cfg.clone();
        let cwd = self.term_cwd.clone();
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let out = actions::execute(&action, &cfg, &cwd);
            let _ = tx.send(Event::ActionDone(msg, card, out));
            ctx.request_repaint();
        });
    }

    pub(super) fn on_action_done(&mut self, msg: usize, card: usize, out: Outcome) {
        if let Some(effect) = out.effect.clone() {
            self.apply_effect(effect);
        }
        if let Some(c) = self.cards_for(msg).get_mut(card) {
            c.status = CardStatus::Done(out);
        }
        self.refresh_files();
        // When every card in the latest reply is settled, report back.
        let latest = self.chat.len().saturating_sub(1);
        if msg == latest && self.claude.auto_continue {
            let cards = self.cards_for(msg).clone();
            let settled = cards
                .iter()
                .all(|c| !matches!(c.status, CardStatus::Pending | CardStatus::Running));
            let gathered = cards.iter().any(|c| {
                matches!(
                    &c.action,
                    Ok(Action::ListFolder { .. })
                        | Ok(Action::Scan { .. })
                        | Ok(Action::RunCommand { .. })
                ) && matches!(c.status, CardStatus::Done(_))
            });
            if settled && gathered {
                self.send_results(msg);
            }
        }
    }

    fn send_results(&mut self, msg: usize) {
        let mut text = String::from("Results of the actions I approved:\n");
        for c in self.cards_for(msg).clone() {
            let name = c
                .action
                .as_ref()
                .map(|a| format!("{}: {}", a.title(), a.detail()))
                .unwrap_or_else(|e| e.clone());
            match c.status {
                CardStatus::Done(o) => text.push_str(&format!(
                    "- {name} -> {} {}\n",
                    if o.ok { "OK" } else { "FAILED" },
                    o.message
                )),
                CardStatus::Denied => text.push_str(&format!("- {name} -> I declined this\n")),
                _ => {}
            }
        }
        self.send_to_claude(text);
    }

    pub(super) fn apply_effect(&mut self, effect: UiEffect) {
        match effect {
            UiEffect::SetTheme(t) => {
                if theme::THEMES.iter().any(|d| d.key == t) {
                    self.cfg.theme = t;
                    self.cfg.accent = None;
                    theme::apply(&self.ctx, &self.cfg);
                    let _ = self.cfg.save();
                }
            }
            UiEffect::SetWallpaper(w) => {
                if theme::WALLPAPERS.iter().any(|(k, _)| *k == w) {
                    self.cfg.wallpaper = w;
                    let _ = self.cfg.save();
                }
            }
            UiEffect::Notify(t, b) => self.push_note(&t, &b),
            UiEffect::PlayMedia(p) => {
                let s = p.to_string_lossy().to_string();
                if s.starts_with("http") {
                    self.play_stream(&s, &s, None);
                } else {
                    self.play_path(p);
                }
            }
            UiEffect::OpenFolder(p) => {
                self.win.files = true;
                self.wm_show(WinId::Files);
                self.go_to(p);
            }
            UiEffect::ShowImage(p) => {
                let _ = exec::open(&p.to_string_lossy());
                self.push_note("Image ready", &p.display().to_string());
            }
        }
    }

    fn generate_image(&mut self, prompt: String) {
        if self.claude.generating_image {
            return;
        }
        self.claude.generating_image = true;
        self.push_note("Generating image…", &prompt);
        let cfg = self.cfg.clone();
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let r = claude::generate_image(&cfg, &prompt).map_err(|e| e.to_string());
            let _ = tx.send(Event::ImageDone(r));
            ctx.request_repaint();
        });
    }

    // --------------------------------------------------------------- UI --

    pub(super) fn claude_window(&mut self, ctx: &egui::Context) {
        let mut open = self.win.claude;
        if !self.wm_visible(WinId::Claude) {
            return;
        }
        let mut run: Option<String> = None;
        let mut approve: Vec<(usize, usize)> = Vec::new();
        let mut deny: Vec<(usize, usize)> = Vec::new();
        let mut send_results: Option<usize> = None;
        let mut load: Option<PathBuf> = None;
        let mut new_chat = false;
        let mut send_text: Option<String> = None;

        self.wm_window(WinId::Claude, RichText::new("Claude").color(theme::CLAUDE), [680.0, 48.0, 640.0, 440.0])
            .open(&mut open)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(claude::backend_status(&self.cfg)).color(theme::DIM));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("New chat").clicked() {
                            new_chat = true;
                        }
                        let hist = ui.menu_button("History", |ui| {
                            if self.claude.history.is_empty() {
                                ui.label("No earlier chats yet.");
                            }
                            for (p, title) in &self.claude.history {
                                if ui.button(title).clicked() {
                                    load = Some(p.clone());
                                    ui.close_menu();
                                }
                            }
                        });
                        if hist.response.clicked() {
                            self.refresh_history();
                        }
                        if ui.small_button("Send system info").clicked() {
                            send_text = Some(format!(
                                "Here is my system status. Anything I should know or fix?\n\n{}",
                                self.snap.summary()
                            ));
                        }
                        ui.checkbox(&mut self.cfg.speak_replies, "Speak").on_hover_text("Read Claude's replies aloud");
                        ui.checkbox(&mut self.claude.auto_continue, "Auto-continue")
                            .on_hover_text("After approved actions finish, send the results back to Claude automatically");
                    });
                });
                ui.separator();
                let input_h = 74.0;
                egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false; 2]).max_height(ui.available_height() - input_h).show(ui, |ui| {
                    if self.chat.is_empty() {
                        ui.label(
                            RichText::new(
                                "Ask anything, or ask Claude to do things: organise your Downloads, scan a folder, \
                                 write a document, play some music, change the theme…\n\
                                 Every action appears as a card. Nothing happens until you press Approve.\n\
                                 Type /image followed by a description to generate a picture on your local server.",
                            )
                            .color(theme::DIM),
                        );
                    }
                    let n = self.chat.len();
                    for i in 0..n {
                        let (role, content) = (self.chat[i].role, self.chat[i].content.clone());
                        bubble(ui, role, &strip_action_blocks(&content));
                        if role != Role::Assistant {
                            continue;
                        }
                        for cmd in claude::extract_commands(&content) {
                            if ui.button(RichText::new(format!("▶ Run: {cmd}")).monospace()).clicked() {
                                run = Some(cmd);
                            }
                        }
                        let cards = self.cards_for(i).clone();
                        if cards.is_empty() {
                            continue;
                        }
                        let pending: Vec<usize> = cards.iter().enumerate().filter(|(_, c)| matches!(c.status, CardStatus::Pending) && c.action.is_ok()).map(|(j, _)| j).collect();
                        for (j, c) in cards.iter().enumerate() {
                            action_card(ui, c, &mut || approve.push((i, j)), &mut || deny.push((i, j)));
                        }
                        ui.horizontal(|ui| {
                            if pending.len() > 1 && ui.button(RichText::new(format!("Approve all {}", pending.len())).strong()).clicked() {
                                approve.extend(pending.iter().map(|j| (i, *j)));
                            }
                            let settled = cards.iter().all(|c| !matches!(c.status, CardStatus::Pending | CardStatus::Running));
                            let any_done = cards.iter().any(|c| matches!(c.status, CardStatus::Done(_)));
                            if settled && any_done && i + 1 == n && ui.small_button("Send results to Claude").clicked() {
                                send_results = Some(i);
                            }
                        });
                    }
                    if let Some(s) = &self.streaming {
                        let shown = if s.is_empty() { "…".to_string() } else { strip_action_blocks(s) };
                        bubble(ui, Role::Assistant, &shown);
                    }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    let r = ui.add(
                        egui::TextEdit::multiline(&mut self.chat_input)
                            .hint_text("Message Claude (Enter to send, Shift+Enter new line, /image to draw)")
                            .desired_rows(2)
                            .desired_width(ui.available_width() - 70.0),
                    );
                    let enter = r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
                    let send = ui.add_enabled(self.streaming.is_none(), egui::Button::new("Send")).clicked();
                    if (enter || send) && self.streaming.is_none() {
                        send_text = Some(std::mem::take(&mut self.chat_input).trim().to_string());
                    }
                });
            });
        self.win.claude = open;

        if new_chat {
            self.new_chat();
        }
        if let Some(p) = load {
            self.load_chat(&p);
        }
        if run.is_some() {
            self.pending_run = run;
        }
        for (i, j) in deny {
            if let Some(c) = self.cards_for(i).get_mut(j) {
                c.status = CardStatus::Denied;
            }
        }
        for (i, j) in approve {
            self.approve(i, j);
        }
        if let Some(i) = send_results {
            self.send_results(i);
        }
        if let Some(t) = send_text.filter(|t| !t.is_empty()) {
            if let Some(prompt) = t.strip_prefix("/image ") {
                self.generate_image(prompt.trim().to_string());
            } else {
                self.send_to_claude(t);
            }
        }
    }
}

/// Hide raw ```action JSON in the chat (the cards show it instead).
pub(super) fn strip_action_blocks(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("```action") {
        out.push_str(&rest[..start]);
        let after = &rest[start..];
        match after[9..].find("```") {
            Some(end) => rest = &after[9 + end + 3..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out.trim().to_string()
}

fn action_card(ui: &mut egui::Ui, c: &Card, approve: &mut dyn FnMut(), deny: &mut dyn FnMut()) {
    egui::Frame::group(ui.style())
        .stroke(egui::Stroke::new(
            1.0_f32,
            theme::accent().linear_multiply(0.4),
        ))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            match &c.action {
                Err(e) => {
                    ui.colored_label(
                        theme::BAD,
                        format!("Claude sent an action OrbitOS couldn't read: {e}"),
                    );
                }
                Ok(a) => {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(a.title())
                                .strong()
                                .color(risk_colour(a.risk())),
                        );
                        let tag = match a.risk() {
                            Risk::Safe => "safe",
                            Risk::Changes => "changes files or settings",
                            Risk::Command => "runs a command",
                        };
                        ui.label(RichText::new(format!("· {tag}")).small().color(theme::DIM));
                    });
                    ui.add(egui::Label::new(RichText::new(a.detail()).monospace()).wrap(true));
                    match &c.status {
                        CardStatus::Pending => {
                            ui.horizontal(|ui| {
                                if ui
                                    .button(RichText::new("Approve").strong().color(theme::OK))
                                    .clicked()
                                {
                                    approve();
                                }
                                if ui.button("Deny").clicked() {
                                    deny();
                                }
                            });
                        }
                        CardStatus::Running => {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label("Working…");
                            });
                        }
                        CardStatus::Denied => {
                            ui.label(RichText::new("Declined").color(theme::DIM));
                        }
                        CardStatus::Done(o) => {
                            let (mark, col) = if o.ok {
                                ("Done", theme::OK)
                            } else {
                                ("Failed", theme::BAD)
                            };
                            let first = o.message.lines().next().unwrap_or("").to_string();
                            ui.label(RichText::new(format!("{mark}: {first}")).color(col));
                            if o.message.lines().count() > 1 {
                                ui.collapsing("Details", |ui| {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&o.message).monospace().small(),
                                        )
                                        .wrap(true),
                                    );
                                });
                            }
                        }
                    }
                }
            }
        });
}
