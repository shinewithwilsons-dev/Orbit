//! Office window: LibreOffice hub, recent documents, PDF conversion, and
//! "Write with Claude" drafts saved as real Word/ODF documents.

use super::*;
use crate::theme::{paint_icon, Glyph};
use orbit_core::office::{self, DocKind};

pub(super) struct OfficeState {
    pub soffice: Option<PathBuf>,
    pub recent: Vec<PathBuf>,
    pub topic: String,
    pub format: usize,
    pub busy: bool,
    pub status: Option<String>,
}

const FORMATS: [(&str, &str); 3] = [
    ("Word (.docx)", "docx"),
    ("OpenDocument (.odt)", "odt"),
    ("PDF", "pdf"),
];

impl OfficeState {
    pub fn new() -> Self {
        Self {
            soffice: office::find_soffice(),
            recent: Vec::new(),
            topic: String::new(),
            format: 0,
            busy: false,
            status: None,
        }
    }
}

impl Desktop {
    pub(super) fn open_office(&mut self) {
        self.win.office = true;
        self.office.soffice = office::find_soffice();
        self.office.recent = office::recent_documents(25);
    }

    pub(super) fn convert_in_background(&mut self, path: PathBuf, format: &'static str) {
        self.notify(format!(
            "Converting {} to {}…",
            path.file_name().unwrap_or_default().to_string_lossy(),
            format.to_uppercase()
        ));
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let r = office::convert(&path, format)
                .map(|p| format!("Saved {}", p.display()))
                .map_err(|e| e.to_string());
            let _ = tx.send(Event::OfficeMsg(r));
            ctx.request_repaint();
        });
    }

    fn start_draft(&mut self) {
        let topic = self.office.topic.trim().to_string();
        if topic.is_empty() || self.office.busy {
            return;
        }
        self.office.busy = true;
        self.office.status = Some("Claude is writing…".into());
        let cfg = self.cfg.clone();
        let format = FORMATS[self.office.format].1;
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let prompt = format!(
                "Write the following as a finished document in Markdown. Start with a '# ' title, \
                 use '## ' section headings, short paragraphs and bullet lists where useful. \
                 Output only the document, with no commentary before or after it.\n\nRequest: {topic}"
            );
            let result = claude::ask(&cfg, &[Message::user(prompt)], &mut |_| {})
                .and_then(|text| {
                    let title = text
                        .lines()
                        .find_map(|l| l.trim().strip_prefix("# ").map(str::to_string))
                        .unwrap_or_else(|| topic.chars().take(50).collect());
                    office::save_draft(&title, &text, format)
                })
                .map_err(|e| e.to_string());
            let _ = tx.send(Event::DraftDone(result));
            ctx.request_repaint();
        });
    }

    pub(super) fn on_draft_done(&mut self, r: Result<PathBuf, String>) {
        self.office.busy = false;
        match r {
            Ok(p) => {
                self.office.status = Some(format!("Saved {}", p.display()));
                let _ = office::open(&p);
                self.office.recent = office::recent_documents(25);
            }
            Err(e) => self.office.status = Some(format!("Draft failed: {e}")),
        }
    }

    pub(super) fn office_window(&mut self, ctx: &egui::Context) {
        let mut open = self.win.office;
        let mut new_doc: Option<DocKind> = None;
        let mut open_doc: Option<PathBuf> = None;
        let mut to_pdf: Option<PathBuf> = None;
        let mut draft = false;
        egui::Window::new(RichText::new("Office").color(theme::OFFICE_BLUE))
            .id(egui::Id::new("office_win"))
            .open(&mut open)
            .default_size([600.0, 420.0])
            .default_pos([744.0, 300.0])
            .show(ctx, |ui| {
                match &self.office.soffice {
                    Some(p) => {
                        ui.label(RichText::new(format!("LibreOffice ready · {}", p.display())).color(theme::SHIELD_GREEN));
                    }
                    None => {
                        ui.label(RichText::new("LibreOffice isn't installed. It's free and open source.").color(theme::WARN));
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(office::install_hint()).monospace());
                            if ui.button("Get LibreOffice").clicked() {
                                let _ = exec::open("https://www.libreoffice.org/download/download/");
                            }
                        });
                    }
                }
                ui.add_space(4.0);
                ui.label(RichText::new("New").strong());
                ui.horizontal(|ui| {
                    for kind in DocKind::ALL {
                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(110.0, 92.0), egui::Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(rect, egui::Rounding::same(8.0), theme::OFFICE_BLUE.linear_multiply(0.18));
                        }
                        let icon = egui::Rect::from_center_size(egui::pos2(rect.center().x, rect.min.y + 34.0), egui::vec2(48.0, 48.0));
                        let ext = match kind {
                            DocKind::Writer => "odt",
                            DocKind::Calc => "ods",
                            DocKind::Impress => "odp",
                            DocKind::Draw => "odg",
                        };
                        paint_icon(ui.painter(), icon, Glyph::File, ext);
                        ui.painter().text(
                            egui::pos2(rect.center().x, rect.max.y - 12.0),
                            egui::Align2::CENTER_CENTER,
                            kind.label(),
                            egui::FontId::proportional(13.0),
                            ui.visuals().text_color(),
                        );
                        if resp.on_hover_text(format!("New LibreOffice {}", kind.app())).clicked() {
                            new_doc = Some(kind);
                        }
                    }
                });
                ui.separator();
                ui.label(RichText::new("Write with Claude").strong().color(theme::CLAUDE));
                ui.add(
                    egui::TextEdit::multiline(&mut self.office.topic)
                        .hint_text("e.g. A one-page mission brief for the CCWL-01 hull test, with risks and next steps")
                        .desired_rows(2)
                        .desired_width(f32::INFINITY),
                );
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_source("draft_fmt")
                        .selected_text(FORMATS[self.office.format].0)
                        .show_ui(ui, |ui| {
                            for (i, (label, _)) in FORMATS.iter().enumerate() {
                                ui.selectable_value(&mut self.office.format, i, *label);
                            }
                        });
                    if ui.add_enabled(!self.office.busy, egui::Button::new("Draft document")).clicked() {
                        draft = true;
                    }
                    if self.office.busy {
                        ui.spinner();
                    }
                });
                if let Some(s) = &self.office.status {
                    ui.label(RichText::new(s).small().color(theme::DIM));
                }
                ui.label(RichText::new(format!("Drafts are saved in {}", office::drafts_dir().display())).small().color(theme::DIM));
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Recent documents").strong());
                    if ui.small_button("⟳").clicked() {
                        self.office.recent = office::recent_documents(25);
                    }
                });
                egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
                    if self.office.recent.is_empty() {
                        ui.label(RichText::new("No documents found in Documents, Downloads or Desktop yet.").color(theme::DIM));
                    }
                    for p in &self.office.recent {
                        ui.horizontal(|ui| {
                            let name = p.file_name().unwrap_or_default().to_string_lossy().to_string();
                            if ui.link(name).on_hover_text(p.display().to_string()).clicked() {
                                open_doc = Some(p.clone());
                            }
                            let is_pdf = p.extension().map(|e| e.eq_ignore_ascii_case("pdf")).unwrap_or(false);
                            if !is_pdf && self.office.soffice.is_some() && ui.small_button("PDF").on_hover_text("Convert to PDF").clicked() {
                                to_pdf = Some(p.clone());
                            }
                        });
                    }
                });
            });
        self.win.office = open;
        if let Some(k) = new_doc {
            match office::new_document(k) {
                Ok(()) => self.notify(format!("Opening LibreOffice {}", k.app())),
                Err(e) => self.notify(e.to_string()),
            }
        }
        if let Some(p) = open_doc {
            if let Err(e) = office::open(&p) {
                self.notify(e.to_string());
            }
        }
        if let Some(p) = to_pdf {
            self.convert_in_background(p, "pdf");
        }
        if draft {
            self.start_draft();
        }
    }
}
