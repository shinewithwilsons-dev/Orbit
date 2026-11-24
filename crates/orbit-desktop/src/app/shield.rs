//! Orbit Shield window: scans, findings, quarantine, real-time watch,
//! ClamAV signature updates, and "Ask Claude" for every finding.

use super::*;
use orbit_core::antivirus::{
    self, Finding, QuarantineItem, ScanOptions, ScanReport, Severity, Watcher,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(PartialEq, Eq, Clone, Copy)]
pub(super) enum ShieldTab {
    Scan,
    Quarantine,
}

pub(super) struct ShieldState {
    pub tab: ShieldTab,
    pub scanning: bool,
    pub count: usize,
    pub current: String,
    pub cancel: Arc<AtomicBool>,
    pub findings: Vec<Finding>,
    pub summary: Option<String>,
    pub notes: Vec<String>,
    pub quarantine: Vec<QuarantineItem>,
    pub custom: String,
    pub watcher: Option<Watcher>,
    pub last_poll: Instant,
    pub clamav: Option<String>,
    pub signatures: usize,
    pub updating: bool,
    pub confirm_delete: Option<QuarantineItem>,
}

impl ShieldState {
    pub fn new(watch: bool) -> Self {
        Self {
            tab: ShieldTab::Scan,
            scanning: false,
            count: 0,
            current: String::new(),
            cancel: Arc::new(AtomicBool::new(false)),
            findings: Vec::new(),
            summary: None,
            notes: Vec::new(),
            quarantine: antivirus::quarantine_list(),
            custom: String::new(),
            watcher: watch.then(|| Watcher::new(antivirus::watch_paths())),
            last_poll: Instant::now(),
            clamav: antivirus::clamav_version(),
            signatures: antivirus::Engines::load().signature_count(),
            updating: false,
            confirm_delete: None,
        }
    }
}

fn sev_colour(s: Severity) -> Color32 {
    match s {
        Severity::High => theme::BAD,
        Severity::Medium => theme::WARN,
        Severity::Low => theme::DIM,
    }
}

impl Desktop {
    pub(super) fn start_scan(&mut self, targets: Vec<PathBuf>) {
        if self.shield.scanning || targets.is_empty() {
            return;
        }
        self.shield.scanning = true;
        self.shield.count = 0;
        self.shield.findings.clear();
        self.shield.summary = None;
        self.shield.notes.clear();
        self.shield.tab = ShieldTab::Scan;
        self.shield.cancel = Arc::new(AtomicBool::new(false));
        let cancel = self.shield.cancel.clone();
        let opts = ScanOptions {
            use_clamav: self.cfg.use_clamav,
            ..Default::default()
        };
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let report = antivirus::scan(&targets, &opts, &cancel, &mut |n, p| {
                let _ = tx.send(Event::ScanProgress(n, p.display().to_string()));
                ctx.request_repaint();
            });
            let _ = tx.send(Event::ScanDone(report));
            ctx.request_repaint();
        });
    }

    pub(super) fn on_scan_done(&mut self, r: ScanReport) {
        self.shield.scanning = false;
        self.shield.summary = Some(format!(
            "{}{} files checked in {:.1}s · {} finding{}{}",
            if r.cancelled { "Stopped · " } else { "" },
            r.files_scanned,
            r.duration.as_secs_f32(),
            r.findings.len(),
            if r.findings.len() == 1 { "" } else { "s" },
            if r.clamav_used {
                " · Orbit engine + ClamAV"
            } else {
                " · Orbit engine"
            }
        ));
        self.shield.notes = r.notes;
        if !r.findings.is_empty() {
            self.notify(format!(
                "Orbit Shield found {} item(s) to review",
                r.findings.len()
            ));
        }
        self.shield.findings = r.findings;
    }

    /// Real-time protection: check new files in Downloads/Desktop.
    pub(super) fn shield_tick(&mut self) {
        if !self.cfg.shield_watch || self.shield.last_poll.elapsed() < Duration::from_secs(4) {
            return;
        }
        self.shield.last_poll = Instant::now();
        let watcher = self
            .shield
            .watcher
            .get_or_insert_with(|| Watcher::new(antivirus::watch_paths()));
        let new_files = watcher.poll();
        if new_files.is_empty() {
            return;
        }
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let opts = ScanOptions {
                use_clamav: false,
                ..Default::default()
            };
            let r = antivirus::scan(&new_files, &opts, &AtomicBool::new(false), &mut |_, _| {});
            if !r.findings.is_empty() {
                let _ = tx.send(Event::Realtime(r.findings));
                ctx.request_repaint();
            }
        });
    }

    pub(super) fn on_realtime(&mut self, found: Vec<Finding>) {
        for f in found {
            if !self
                .shield
                .findings
                .iter()
                .any(|x| x.path == f.path && x.threat == f.threat)
            {
                self.notify(format!(
                    "Orbit Shield: {} in {}",
                    f.threat,
                    f.path.file_name().unwrap_or_default().to_string_lossy()
                ));
                self.shield.findings.insert(0, f);
            }
        }
        self.win.shield = true;
        self.shield.tab = ShieldTab::Scan;
    }

    fn quarantine_finding(&mut self, f: &Finding) {
        match antivirus::quarantine(&f.path, &f.threat) {
            Ok(_) => {
                self.shield.findings.retain(|x| x.path != f.path);
                self.shield.quarantine = antivirus::quarantine_list();
                self.notify(format!(
                    "Quarantined {}",
                    f.path.file_name().unwrap_or_default().to_string_lossy()
                ));
                self.refresh_files();
            }
            Err(e) => self.notify(format!("Quarantine failed: {e}")),
        }
    }

    pub(super) fn shield_window(&mut self, ctx: &egui::Context) {
        let mut open = self.win.shield;
        let mut quarantine: Vec<Finding> = Vec::new();
        let mut ignore: Option<usize> = None;
        let mut ask: Option<String> = None;
        let mut scan: Option<Vec<PathBuf>> = None;
        let mut restore: Option<String> = None;
        let mut update = false;
        let mut save_cfg = false;

        egui::Window::new(RichText::new("Orbit Shield").color(theme::SHIELD_GREEN))
            .id(egui::Id::new("shield_win"))
            .open(&mut open)
            .default_size([600.0, 400.0])
            .default_pos([744.0, 44.0])
            .show(ctx, |ui| {
                // Status header.
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(46.0, 46.0), egui::Sense::hover());
                    theme::paint_icon(ui.painter(), rect, theme::Glyph::Shield, "");
                    ui.vertical(|ui| {
                        let threats = self.shield.findings.iter().filter(|f| f.severity >= Severity::Medium).count();
                        let (text, col) = if self.shield.scanning {
                            ("Scanning…".to_string(), theme::ACCENT)
                        } else if threats > 0 {
                            (format!("{threats} threat(s) need attention"), theme::BAD)
                        } else if !self.shield.findings.is_empty() {
                            ("Low-risk items to review".to_string(), theme::WARN)
                        } else {
                            ("You're protected".to_string(), theme::SHIELD_GREEN)
                        };
                        ui.label(RichText::new(text).size(20.0).strong().color(col));
                        ui.label(
                            RichText::new(format!(
                                "Orbit engine: {} signature(s) + heuristics · {}",
                                self.shield.signatures,
                                self.shield.clamav.clone().unwrap_or_else(|| "ClamAV not installed".into())
                            ))
                            .color(theme::DIM),
                        );
                    });
                });
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut self.cfg.shield_watch, "Real-time watch (Downloads, Desktop)").changed() {
                        save_cfg = true;
                    }
                    if ui.checkbox(&mut self.cfg.use_clamav, "Use ClamAV").changed() {
                        save_cfg = true;
                    }
                    let label = if self.shield.updating { "Updating…" } else { "Update signatures" };
                    if ui.add_enabled(!self.shield.updating && self.shield.clamav.is_some(), egui::Button::new(label)).clicked() {
                        update = true;
                    }
                });
                if self.shield.clamav.is_none() {
                    ui.label(RichText::new(format!("For millions more signatures install ClamAV: {}", antivirus::clamav_install_hint())).small().color(theme::DIM));
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.shield.tab, ShieldTab::Scan, "Scan");
                    ui.selectable_value(&mut self.shield.tab, ShieldTab::Quarantine, format!("Quarantine ({})", self.shield.quarantine.len()));
                });
                ui.separator();

                match self.shield.tab {
                    ShieldTab::Scan => {
                        ui.horizontal(|ui| {
                            let idle = !self.shield.scanning;
                            if ui.add_enabled(idle, egui::Button::new("Quick scan")).on_hover_text("Downloads, Desktop, Documents and temp").clicked() {
                                scan = Some(antivirus::quick_scan_paths());
                            }
                            if ui.add_enabled(idle, egui::Button::new("Full scan")).on_hover_text("Your whole home folder").clicked() {
                                scan = icons::dirs_home().map(|h| vec![h]);
                            }
                            ui.add(egui::TextEdit::singleline(&mut self.shield.custom).hint_text("folder or file to scan").desired_width(200.0));
                            if ui.add_enabled(idle && !self.shield.custom.trim().is_empty(), egui::Button::new("Scan")).clicked() {
                                scan = Some(vec![orbit_core::expand_tilde(self.shield.custom.trim())]);
                            }
                            if self.shield.scanning && ui.button("Stop").clicked() {
                                self.shield.cancel.store(true, Ordering::Relaxed);
                            }
                        });
                        if self.shield.scanning {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(format!("{} files checked", self.shield.count));
                            });
                            ui.add(egui::Label::new(RichText::new(&self.shield.current).small().color(theme::DIM)).truncate(true));
                            if self.cfg.use_clamav && self.shield.clamav.is_some() {
                                ui.label(RichText::new("ClamAV runs after the Orbit engine; loading its database can take a little while.").small().color(theme::DIM));
                            }
                        }
                        if let Some(s) = &self.shield.summary {
                            ui.label(RichText::new(s).strong());
                        }
                        for n in &self.shield.notes {
                            ui.label(RichText::new(n).small().color(theme::WARN));
                        }
                        if !self.shield.findings.is_empty() {
                            ui.horizontal(|ui| {
                                if ui.button("Quarantine all high-risk").clicked() {
                                    quarantine = self.shield.findings.iter().filter(|f| f.severity == Severity::High).cloned().collect();
                                }
                            });
                        }
                        egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
                            for (i, f) in self.shield.findings.iter().enumerate() {
                                egui::Frame::group(ui.style()).show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(f.severity.label()).strong().color(sev_colour(f.severity)));
                                        ui.label(RichText::new(&f.threat).strong());
                                        ui.label(RichText::new(format!("· {}", f.engine)).color(theme::DIM));
                                    });
                                    ui.add(egui::Label::new(RichText::new(f.path.display().to_string()).monospace().small()).truncate(true))
                                        .on_hover_text(f.path.display().to_string());
                                    ui.label(RichText::new(&f.detail).color(theme::DIM));
                                    ui.horizontal(|ui| {
                                        if ui.button("Quarantine").clicked() {
                                            quarantine.push(f.clone());
                                        }
                                        if ui.button(RichText::new("Ask Claude").color(theme::CLAUDE)).clicked() {
                                            ask = Some(f.describe());
                                        }
                                        if ui.button("Ignore").clicked() {
                                            ignore = Some(i);
                                        }
                                    });
                                });
                            }
                            if self.shield.findings.is_empty() && !self.shield.scanning && self.shield.summary.is_some() {
                                ui.label(RichText::new("No threats found.").color(theme::SHIELD_GREEN));
                            }
                        });
                    }
                    ShieldTab::Quarantine => {
                        ui.label(RichText::new("Quarantined files are encoded so they can't run. Restore puts a file back exactly where it was.").color(theme::DIM));
                        egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
                            if self.shield.quarantine.is_empty() {
                                ui.label("Quarantine is empty.");
                            }
                            for q in &self.shield.quarantine {
                                egui::Frame::group(ui.style()).show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.label(RichText::new(&q.threat).strong());
                                    ui.add(egui::Label::new(RichText::new(q.original.display().to_string()).monospace().small()).truncate(true));
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(human_bytes(q.size)).color(theme::DIM));
                                        if ui.button("Restore").clicked() {
                                            restore = Some(q.id.clone());
                                        }
                                        if ui.button(RichText::new("Delete forever").color(theme::BAD)).clicked() {
                                            self.shield.confirm_delete = Some(q.clone());
                                        }
                                    });
                                });
                            }
                        });
                    }
                }
            });
        self.win.shield = open;

        if save_cfg {
            if self.cfg.shield_watch && self.shield.watcher.is_none() {
                self.shield.watcher = Some(Watcher::new(antivirus::watch_paths()));
            }
            let _ = self.cfg.save();
        }
        if let Some(t) = scan {
            self.start_scan(t);
        }
        for f in quarantine {
            self.quarantine_finding(&f);
        }
        if let Some(i) = ignore {
            if i < self.shield.findings.len() {
                self.shield.findings.remove(i);
            }
        }
        if let Some(q) = ask {
            self.send_to_claude(q);
        }
        if let Some(id) = restore {
            match antivirus::restore(&id) {
                Ok(p) => self.notify(format!("Restored {}", p.display())),
                Err(e) => self.notify(format!("Restore failed: {e}")),
            }
            self.shield.quarantine = antivirus::quarantine_list();
            self.refresh_files();
        }
        if update {
            self.shield.updating = true;
            let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
            std::thread::spawn(move || {
                let _ = tx.send(Event::ShieldUpdate(
                    antivirus::update_clamav().map_err(|e| e.to_string()),
                ));
                ctx.request_repaint();
            });
        }
        self.confirm_delete_dialog(ctx);
    }

    fn confirm_delete_dialog(&mut self, ctx: &egui::Context) {
        let Some(q) = self.shield.confirm_delete.clone() else {
            return;
        };
        let mut decision = None;
        egui::Window::new("Delete permanently?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(format!("{} ({})", q.original.display(), q.threat));
                ui.label(RichText::new("This can't be undone.").color(theme::BAD));
                ui.horizontal(|ui| {
                    if ui
                        .button(RichText::new("Delete").color(theme::BAD))
                        .clicked()
                    {
                        decision = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        decision = Some(false);
                    }
                });
            });
        match decision {
            Some(true) => {
                self.shield.confirm_delete = None;
                match antivirus::delete_quarantined(&q.id) {
                    Ok(()) => self.notify("Deleted permanently"),
                    Err(e) => self.notify(format!("Delete failed: {e}")),
                }
                self.shield.quarantine = antivirus::quarantine_list();
            }
            Some(false) => self.shield.confirm_delete = None,
            None => {}
        }
    }
}
