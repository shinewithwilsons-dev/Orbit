//! Claude folders window: one folder each for Skills, Plugins, Projects,
//! Artifacts and Scheduled tasks. Each folder lives under the Orbit data
//! directory and opens in the system file manager.

use super::*;

/// (section title, folder name under claude/, one-line description)
const SECTIONS: [(&str, &str, &str); 5] = [
    ("Skills", "skills", "Reusable instructions Claude can follow."),
    ("Plugins", "plugins", "Add-ons that extend Orbit and Claude."),
    ("Projects", "projects", "Work grouped by project."),
    ("Artifacts", "artifacts", "Pages, documents and files Claude made for you."),
    ("Scheduled", "scheduled", "Tasks that run later or on a schedule."),
];

struct Item {
    name: String,
    path: PathBuf,
    is_dir: bool,
}

/// The folder for one section, created if missing, and its contents
/// (folders first, then files, each alphabetically).
fn section_folder(dir: &str) -> (PathBuf, Vec<Item>) {
    let path = Config::data_dir().join("claude").join(dir);
    let _ = std::fs::create_dir_all(&path);
    let mut items: Vec<Item> = std::fs::read_dir(&path)
        .map(|rd| {
            rd.flatten()
                .map(|e| {
                    let p = e.path();
                    Item {
                        name: e.file_name().to_string_lossy().into_owned(),
                        is_dir: p.is_dir(),
                        path: p,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    items.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    (path, items)
}

impl Desktop {
    pub(super) fn folders_window(&mut self, ctx: &egui::Context) {
        let mut open = self.win.folders;
        if !self.wm_visible(WinId::Folders) {
            return;
        }
        let mut open_path: Option<PathBuf> = None;
        self.wm_window(
            WinId::Folders,
            RichText::new("Claude folders").color(theme::CLAUDE),
            [240.0, 60.0, 380.0, 470.0],
        )
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(RichText::new("Everything Claude keeps for you, in one place.").color(theme::DIM));
            ui.separator();
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    for (title, dir, hint) in SECTIONS {
                        let (path, items) = section_folder(dir);
                        egui::CollapsingHeader::new(format!("{title}  ({})", items.len()))
                            .id_source(dir)
                            .default_open(true)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(hint).color(theme::DIM));
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui
                                                .small_button("Open folder")
                                                .on_hover_text(path.display().to_string())
                                                .clicked()
                                            {
                                                open_path = Some(path.clone());
                                            }
                                        },
                                    );
                                });
                                if items.is_empty() {
                                    ui.label(
                                        RichText::new("Nothing here yet.").color(theme::DIM),
                                    );
                                }
                                for item in &items {
                                    let mark = if item.is_dir { "▸" } else { "•" };
                                    if ui
                                        .selectable_label(false, format!("{mark} {}", item.name))
                                        .on_hover_text(item.path.display().to_string())
                                        .clicked()
                                    {
                                        open_path = Some(item.path.clone());
                                    }
                                }
                                ui.add_space(6.0);
                            });
                    }
                });
        });
        self.win.folders = open;

        if let Some(p) = open_path {
            if let Err(e) = exec::open(&p.to_string_lossy()) {
                self.toast = Some((format!("Could not open that folder: {e}"), Instant::now()));
            }
        }
    }
}
