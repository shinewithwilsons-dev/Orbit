//! App Store window: one-click installs of free, open-source apps.

use super::icons::WinId;
use super::*;
use orbit_core::store::{self, StoreApp};
use std::collections::HashSet;

#[derive(Default)]
pub(super) struct StoreState {
    pub category: Option<&'static str>,
    pub filter: String,
    pub installed: HashSet<&'static str>,
    pub checked: bool,
    pub checking: bool,
}

impl Desktop {
    pub(super) fn open_store(&mut self) {
        self.win.store = true;
        self.refresh_store();
    }

    pub(super) fn refresh_store(&mut self) {
        if self.store.checking {
            return;
        }
        self.store.checking = true;
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let set: HashSet<&'static str> = store::catalog()
                .iter()
                .filter(|a| store::is_installed(a))
                .map(|a| a.name)
                .collect();
            let _ = tx.send(Event::StoreChecked(set));
            ctx.request_repaint();
        });
    }

    /// Ask to run an install command (shown in the confirm dialog, then run in Terminal).
    pub(super) fn request_install(&mut self, app: &StoreApp) {
        match store::install_command(app) {
            Some(cmd) => {
                self.pending_run = Some(cmd);
                self.push_note("App Store", &format!("Confirm to install {}", app.name));
            }
            None => {
                let _ = exec::open(app.homepage);
                self.push_note(
                    "App Store",
                    &format!(
                        "{} has no package for this system; opened its website",
                        app.name
                    ),
                );
            }
        }
    }

    pub(super) fn store_window(&mut self, ctx: &egui::Context) {
        let mut open = self.win.store;
        if !self.wm_visible(WinId::Store) {
            return;
        }
        let mut install: Option<StoreApp> = None;
        let mut refresh = false;
        self.wm_window(WinId::Store, RichText::new("App Store").color(theme::accent()), [300.0, 70.0, 720.0, 520.0])
            .open(&mut open)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Free & open source apps").strong());
                    ui.label(
                        RichText::new(if cfg!(windows) { "via winget" } else if cfg!(target_os = "macos") { "via Homebrew" } else { "via Flatpak / apt" })
                            .color(theme::DIM),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.store.checking {
                            ui.spinner();
                        } else if ui.small_button("⟳").on_hover_text("Re-check what's installed").clicked() {
                            refresh = true;
                        }
                        ui.add(egui::TextEdit::singleline(&mut self.store.filter).hint_text("Search…").desired_width(180.0));
                    });
                });
                ui.horizontal_wrapped(|ui| {
                    if ui.selectable_label(self.store.category.is_none(), "All").clicked() {
                        self.store.category = None;
                    }
                    for c in store::categories() {
                        if ui.selectable_label(self.store.category == Some(c), c).clicked() {
                            self.store.category = Some(c);
                        }
                    }
                });
                ui.separator();
                let q = self.store.filter.to_lowercase();
                let apps: Vec<StoreApp> = store::catalog()
                    .into_iter()
                    .filter(|a| self.store.category.map(|c| a.category == c).unwrap_or(true))
                    .filter(|a| q.is_empty() || a.name.to_lowercase().contains(&q) || a.about.to_lowercase().contains(&q))
                    .collect();
                egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
                    let cols = ((ui.available_width() / 330.0).floor() as usize).max(1);
                    egui::Grid::new("store_grid").num_columns(cols).spacing([10.0, 10.0]).show(ui, |ui| {
                        for (i, a) in apps.iter().enumerate() {
                            egui::Frame::group(ui.style()).rounding(egui::Rounding::same(10.0)).show(ui, |ui| ui.vertical(|ui| {
                                ui.set_width(310.0);
                                ui.set_min_height(92.0);
                                ui.horizontal(|ui| {
                                    let (rect, _) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
                                    let col = theme::file_colour(match a.category {
                                        "Media" => "mp3",
                                        "Office" => "docx",
                                        "Security" | "Utilities" => "xlsx",
                                        "Developer" => "exe",
                                        "Graphics" => "png",
                                        _ => "pptx",
                                    });
                                    ui.painter().rect_filled(rect, egui::Rounding::same(10.0), col);
                                    let initial: String = a.name.chars().take(1).collect();
                                    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, initial, egui::FontId::proportional(20.0), Color32::WHITE);
                                    ui.vertical(|ui| {
                                        ui.label(RichText::new(a.name).strong());
                                        ui.label(RichText::new(a.category).small().color(theme::DIM));
                                    });
                                });
                                ui.label(RichText::new(a.about).small());
                                ui.horizontal(|ui| {
                                    if self.store.installed.contains(a.name) {
                                        ui.label(RichText::new("Installed").color(theme::OK));
                                    } else if store::install_command(a).is_some() {
                                        if ui.button(RichText::new("Install").strong()).clicked() {
                                            install = Some(a.clone());
                                        }
                                    } else {
                                        ui.label(RichText::new("Manual install").small().color(theme::DIM));
                                    }
                                    if ui.link("Website").clicked() {
                                        let _ = exec::open(a.homepage);
                                    }
                                });
                            }));
                            if (i + 1) % cols == 0 {
                                ui.end_row();
                            }
                        }
                    });
                });
                ui.label(RichText::new("Installs run in Terminal after you confirm. Your system may ask for your password.").small().color(theme::DIM));
            });
        self.win.store = open;
        if refresh {
            self.refresh_store();
        }
        if let Some(a) = install {
            self.request_install(&a);
        }
    }
}
