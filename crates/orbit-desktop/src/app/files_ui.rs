//! Files window: click-style icon grid (or list), with actions for Claude,
//! Orbit Shield, Media and Office in the right-click menu.

use super::*;
use crate::theme::{paint_icon, Glyph};
use orbit_core::{media, office};

enum FileAction {
    Open(files::Entry),
    Navigate(PathBuf),
    Ask(PathBuf),
    Scan(PathBuf),
    Play(PathBuf),
    OfficeOpen(PathBuf),
    ToPdf(PathBuf),
}

fn ext_of(p: &std::path::Path) -> String {
    p.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

impl Desktop {
    pub(super) fn files_window(&mut self, ctx: &egui::Context) {
        let mut open = self.win.files;
        let mut actions: Vec<FileAction> = Vec::new();
        if !self.wm_visible(WinId::Files) {
            return;
        }
        self.wm_window(WinId::Files, "Files", [224.0, 44.0, 500.0, 320.0])
            .open(&mut open)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("⬆ Up").on_hover_text("Parent folder").clicked() {
                        if let Some(p) = self.cwd.parent() {
                            actions.push(FileAction::Navigate(p.to_path_buf()));
                        }
                    }
                    if ui.button("Home").clicked() {
                        if let Some(h) = icons::dirs_home() {
                            actions.push(FileAction::Navigate(h));
                        }
                    }
                    let r = ui.add(egui::TextEdit::singleline(&mut self.path_edit).desired_width(ui.available_width() - 210.0));
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        actions.push(FileAction::Navigate(orbit_core::expand_tilde(&self.path_edit)));
                    }
                    ui.selectable_value(&mut self.files_grid, true, "Icons");
                    ui.selectable_value(&mut self.files_grid, false, "List");
                    if ui.checkbox(&mut self.show_hidden, "Hidden").changed() {
                        actions.push(FileAction::Navigate(self.cwd.clone()));
                    }
                    if ui.button("⟳").on_hover_text("Refresh").clicked() {
                        actions.push(FileAction::Navigate(self.cwd.clone()));
                    }
                });
                ui.separator();
                if let Some(e) = &self.files_err {
                    ui.colored_label(theme::BAD, e);
                }
                let footer = 22.0;
                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .max_height(ui.available_height() - footer)
                    .show(ui, |ui| {
                        if self.entries.is_empty() && self.files_err.is_none() {
                            ui.label(RichText::new("This folder is empty.").color(theme::DIM));
                        }
                        if self.files_grid {
                            self.files_grid_view(ui, &mut actions);
                        } else {
                            self.files_list_view(ui, &mut actions);
                        }
                    });
                ui.label(
                    RichText::new(format!(
                        "{} items · click to select, double-click to open, right-click for Claude, Shield, Media and Office",
                        self.entries.len()
                    ))
                    .small()
                    .color(theme::DIM),
                );
            });
        self.win.files = open;
        for a in actions {
            self.apply_file_action(a);
        }
    }

    fn files_grid_view(&mut self, ui: &mut egui::Ui, actions: &mut Vec<FileAction>) {
        let cell = egui::vec2(96.0, 92.0);
        let cols = ((ui.available_width() / cell.x).floor() as usize).max(1);
        let entries = self.entries.clone();
        for row in entries.chunks(cols) {
            ui.horizontal(|ui| {
                for e in row {
                    let (rect, resp) = ui.allocate_exact_size(cell, egui::Sense::click());
                    let selected = self.file_selected.as_ref() == Some(&e.path);
                    if selected || resp.hovered() {
                        ui.painter().rect_filled(
                            rect.shrink(2.0),
                            egui::Rounding::same(8.0),
                            theme::accent().linear_multiply(if selected { 0.35 } else { 0.15 }),
                        );
                    }
                    let icon = egui::Rect::from_center_size(
                        egui::pos2(rect.center().x, rect.min.y + 32.0),
                        egui::vec2(48.0, 48.0),
                    );
                    if e.is_dir {
                        paint_icon(ui.painter(), icon, Glyph::Folder, "");
                    } else {
                        let ext: String = ext_of(&e.path).chars().take(4).collect();
                        paint_icon(ui.painter(), icon, Glyph::File, &ext);
                    }
                    let name: String = if e.name.chars().count() > 13 {
                        format!("{}…", e.name.chars().take(12).collect::<String>())
                    } else {
                        e.name.clone()
                    };
                    ui.painter().text(
                        egui::pos2(rect.center().x, rect.max.y - 14.0),
                        egui::Align2::CENTER_CENTER,
                        name,
                        egui::FontId::proportional(12.5),
                        ui.visuals().text_color(),
                    );
                    let resp = resp.on_hover_text(format!(
                        "{}\n{}",
                        e.name,
                        if e.is_dir {
                            "Folder".to_string()
                        } else {
                            human_bytes(e.size)
                        }
                    ));
                    self.file_interactions(&resp, e, actions);
                }
            });
        }
    }

    fn files_list_view(&mut self, ui: &mut egui::Ui, actions: &mut Vec<FileAction>) {
        let entries = self.entries.clone();
        egui::Grid::new("files_list")
            .num_columns(2)
            .striped(true)
            .min_col_width(80.0)
            .show(ui, |ui| {
                for e in &entries {
                    let label = if e.is_dir {
                        RichText::new(format!("▸ {}", e.name)).color(theme::accent())
                    } else {
                        RichText::new(&e.name)
                    };
                    let selected = self.file_selected.as_ref() == Some(&e.path);
                    let resp = ui.selectable_label(selected, label);
                    self.file_interactions(&resp, e, actions);
                    ui.label(
                        RichText::new(if e.is_dir {
                            "folder".into()
                        } else {
                            human_bytes(e.size)
                        })
                        .color(theme::DIM),
                    );
                    ui.end_row();
                }
            });
    }

    fn file_interactions(
        &mut self,
        resp: &egui::Response,
        e: &files::Entry,
        actions: &mut Vec<FileAction>,
    ) {
        if resp.clicked() {
            self.file_selected = Some(e.path.clone());
        }
        if resp.double_clicked() {
            actions.push(FileAction::Open(e.clone()));
        }
        let ext = ext_of(&e.path);
        let is_media = media::kind_of(&e.path).is_some();
        let is_doc = office::DOC_EXT.contains(&ext.as_str());
        resp.context_menu(|ui| {
            if ui.button("Open").clicked() {
                actions.push(FileAction::Open(e.clone()));
                ui.close_menu();
            }
            if e.is_dir {
                if ui.button("Scan folder with Orbit Shield").clicked() {
                    actions.push(FileAction::Scan(e.path.clone()));
                    ui.close_menu();
                }
            } else {
                if ui.button("Ask Claude about this file").clicked() {
                    actions.push(FileAction::Ask(e.path.clone()));
                    ui.close_menu();
                }
                if ui.button("Scan with Orbit Shield").clicked() {
                    actions.push(FileAction::Scan(e.path.clone()));
                    ui.close_menu();
                }
                if is_media && ui.button("Play in Media").clicked() {
                    actions.push(FileAction::Play(e.path.clone()));
                    ui.close_menu();
                }
                if is_doc {
                    if ui.button("Open in LibreOffice").clicked() {
                        actions.push(FileAction::OfficeOpen(e.path.clone()));
                        ui.close_menu();
                    }
                    if ext != "pdf" && ui.button("Convert to PDF").clicked() {
                        actions.push(FileAction::ToPdf(e.path.clone()));
                        ui.close_menu();
                    }
                }
            }
            if ui.button("Copy path").clicked() {
                ui.ctx()
                    .output_mut(|o| o.copied_text = e.path.display().to_string());
                ui.close_menu();
            }
        });
    }

    fn apply_file_action(&mut self, a: FileAction) {
        match a {
            FileAction::Navigate(p) => self.go_to(p),
            FileAction::Open(e) => {
                if e.is_dir {
                    self.go_to(e.path);
                } else if media::kind_of(&e.path).is_some() {
                    self.play_path(e.path);
                } else if office::DOC_EXT.contains(&ext_of(&e.path).as_str())
                    && ext_of(&e.path) != "md"
                {
                    if let Err(err) = office::open(&e.path) {
                        self.notify(err.to_string());
                    }
                } else if let Err(err) = exec::open(&e.path.to_string_lossy()) {
                    self.notify(format!("Could not open: {err}"));
                }
            }
            FileAction::Ask(p) => self.ask_about_file(&p),
            FileAction::Scan(p) => {
                self.win.shield = true;
                self.start_scan(vec![p]);
            }
            FileAction::Play(p) => self.play_path(p),
            FileAction::OfficeOpen(p) => {
                if let Err(err) = office::open(&p) {
                    self.notify(err.to_string());
                }
            }
            FileAction::ToPdf(p) => self.convert_in_background(p, "pdf"),
        }
    }
}
