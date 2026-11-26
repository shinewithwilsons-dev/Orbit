//! Orbit Start menu: a Start-style menu from the Orbit button. Search,
//! ten pinned Orbit apps, recent documents, Claude's status, Hide and Quit.

use super::icons::WinId;
use super::*;
use crate::theme::{paint_icon, Glyph};

#[derive(Default)]
pub(super) struct StartMenu {
    pub open: bool,
    pub query: String,
    pub recent: Vec<PathBuf>,
    pub opened_at: Option<Instant>,
}

#[derive(Clone, Copy)]
enum Pin {
    Win(WinId, Glyph),
    Apps,
}

const PINS: [(&str, Pin); 12] = [
    ("Files", Pin::Win(WinId::Files, Glyph::Folder)),
    ("Terminal", Pin::Win(WinId::Terminal, Glyph::Terminal)),
    ("Claude folders", Pin::Win(WinId::Folders, Glyph::Folder)),
    ("Claude", Pin::Win(WinId::Claude, Glyph::Claude)),
    ("Shield", Pin::Win(WinId::Shield, Glyph::Shield)),
    ("Media", Pin::Win(WinId::Media, Glyph::Media)),
    ("Office", Pin::Win(WinId::Office, Glyph::Office)),
    ("App Store", Pin::Win(WinId::Store, Glyph::Apps)),
    ("Monitor", Pin::Win(WinId::Monitor, Glyph::Monitor)),
    ("Hardware setup", Pin::Win(WinId::Hardware, Glyph::Monitor)),
    ("Settings", Pin::Win(WinId::Settings, Glyph::Settings)),
    ("All apps", Pin::Apps),
];

impl Desktop {
    pub(super) fn toggle_start(&mut self) {
        if self.start.open {
            self.start.open = false;
            return;
        }
        self.start.open = true;
        self.start.query.clear();
        self.start.opened_at = Some(Instant::now());
        self.start.recent = orbit_core::office::recent_documents(5);
    }

    pub(super) fn start_menu_ui(&mut self, ctx: &egui::Context) {
        if !self.start.open {
            return;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.start.open = false;
            return;
        }
        let mut pick: Option<Pin> = None;
        let mut open_doc: Option<PathBuf> = None;
        let mut hide = false;
        let mut quit = false;
        let mut search: Option<String> = None;
        let accent = theme::accent();
        let area = egui::Area::new(egui::Id::new("start_menu"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::LEFT_BOTTOM, [8.0, -54.0])
            .show(ctx, |ui| {
                egui::Frame::window(ui.style())
                    .stroke(egui::Stroke::new(1.5_f32, accent))
                    .rounding(egui::Rounding::same(12.0))
                    .inner_margin(egui::Margin::same(14.0))
                    .show(ui, |ui| {
                        ui.set_width(372.0);
                        let r = ui.add(
                            egui::TextEdit::singleline(&mut self.start.query)
                                .hint_text("Search apps, files, or ask Claude")
                                .desired_width(f32::INFINITY),
                        );
                        if self
                            .start
                            .opened_at
                            .map(|t| t.elapsed() < Duration::from_millis(150))
                            .unwrap_or(false)
                        {
                            r.request_focus();
                        }
                        if r.changed() && !self.start.query.trim().is_empty() {
                            search = Some(self.start.query.clone());
                        }
                        ui.add_space(8.0);
                        ui.label(RichText::new("Pinned").strong());
                        let tile = egui::vec2(72.0, 70.0);
                        for row in PINS.chunks(5) {
                            ui.horizontal(|ui| {
                                for (label, pin) in row {
                                    let (rect, resp) =
                                        ui.allocate_exact_size(tile, egui::Sense::click());
                                    if resp.hovered() {
                                        ui.painter().rect_filled(
                                            rect,
                                            egui::Rounding::same(8.0),
                                            accent.linear_multiply(0.15),
                                        );
                                    }
                                    let glyph = match pin {
                                        Pin::Win(_, g) => *g,
                                        Pin::Apps => Glyph::Apps,
                                    };
                                    let icon = egui::Rect::from_center_size(
                                        egui::pos2(rect.center().x, rect.min.y + 24.0),
                                        egui::vec2(36.0, 36.0),
                                    );
                                    paint_icon(ui.painter(), icon, glyph, "");
                                    ui.painter().text(
                                        egui::pos2(rect.center().x, rect.max.y - 10.0),
                                        egui::Align2::CENTER_CENTER,
                                        *label,
                                        egui::FontId::proportional(12.0),
                                        ui.visuals().text_color(),
                                    );
                                    if resp.clicked() {
                                        pick = Some(*pin);
                                    }
                                }
                            });
                        }
                        ui.add_space(6.0);
                        ui.label(RichText::new("Recent").strong());
                        if self.start.recent.is_empty() {
                            ui.label(RichText::new("No recent documents yet.").color(theme::DIM));
                        }
                        for p in &self.start.recent {
                            let name = p
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();
                            if ui
                                .add(egui::Label::new(name).sense(egui::Sense::click()))
                                .on_hover_text(p.display().to_string())
                                .clicked()
                            {
                                open_doc = Some(p.clone());
                            }
                        }
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(claude::backend_status(&self.cfg))
                                    .small()
                                    .color(theme::DIM),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .button("Quit")
                                        .on_hover_text("Quit Orbit (Ctrl+Q)")
                                        .clicked()
                                    {
                                        quit = true;
                                    }
                                    if ui
                                        .button("Hide")
                                        .on_hover_text(format!(
                                            "Hide to the tray ({})",
                                            tray::hotkey_label()
                                        ))
                                        .clicked()
                                    {
                                        hide = true;
                                    }
                                },
                            );
                        });
                    });
            });
        // A click outside closes the menu (the Orbit button toggles it itself).
        let clicked_outside = ctx.input(|i| i.pointer.any_click())
            && ctx
                .input(|i| i.pointer.interact_pos())
                .map(|p| !area.response.rect.contains(p))
                .unwrap_or(false)
            && self
                .start
                .opened_at
                .map(|t| t.elapsed() > Duration::from_millis(200))
                .unwrap_or(true);
        if clicked_outside && !self.start_button_clicked {
            self.start.open = false;
        }
        if let Some(q) = search {
            self.start.open = false;
            self.open_cmdbar();
            self.cmd.query = q;
        }
        if let Some(pin) = pick {
            self.start.open = false;
            match pin {
                Pin::Win(w, _) => self.open_win(w),
                Pin::Apps => self.show_launcher = true,
            }
        }
        if let Some(p) = open_doc {
            self.start.open = false;
            if let Err(e) = orbit_core::office::open(&p) {
                self.notify(e.to_string());
            }
        }
        if hide {
            self.start.open = false;
            self.hide_to_tray();
        }
        if quit {
            self.quit();
        }
    }
}
