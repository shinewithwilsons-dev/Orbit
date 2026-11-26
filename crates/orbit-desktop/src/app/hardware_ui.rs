//! Hardware setup window: scans the computer for devices without a working
//! driver and lists a fix for each. Fixes go through the same confirm-and-run
//! dialog as other commands in OrbitOS, so nothing changes without approval.

use super::*;
use orbit_core::hardware::{self, Device, Level, State};

#[derive(Default)]
pub(super) struct HardwareState {
    pub report: Option<hardware::Report>,
    pub scanning: bool,
}

impl Desktop {
    pub(super) fn open_hardware(&mut self) {
        self.win.hardware = true;
        if self.hardware.report.is_none() {
            self.start_hardware_scan();
        }
    }

    pub(super) fn start_hardware_scan(&mut self) {
        if self.hardware.scanning {
            return;
        }
        self.hardware.scanning = true;
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Event::HardwareScan(Box::new(hardware::scan())));
            ctx.request_repaint();
        });
    }

    pub(super) fn hardware_window(&mut self, ctx: &egui::Context) {
        let mut open = self.win.hardware;
        if !self.wm_visible(WinId::Hardware) {
            return;
        }
        let mut rescan = false;
        let mut fix_cmd: Option<String> = None;
        let mut search: Option<String> = None;
        let scanning = self.hardware.scanning;

        self.wm_window(
            WinId::Hardware,
            RichText::new("Hardware setup").color(theme::accent()),
            [260.0, 50.0, 600.0, 540.0],
        )
        .open(&mut open)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!scanning, egui::Button::new("Scan system"))
                    .clicked()
                {
                    rescan = true;
                }
                if scanning {
                    ui.spinner();
                    ui.label(RichText::new("Scanning…").color(theme::DIM));
                }
            });
            ui.separator();
            let Some(rep) = self.hardware.report.as_ref() else {
                ui.label(RichText::new("Scanning your hardware…").color(theme::DIM));
                return;
            };
            ui.label(RichText::new(&rep.system).color(theme::DIM));
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    if rep.findings.is_empty() {
                        ui.colored_label(theme::OK, "No hardware or driver problems found.");
                    }
                    for f in &rep.findings {
                        let colour = match f.level {
                            Level::Bad => theme::BAD,
                            Level::Warn => theme::WARN,
                            Level::Info => theme::DIM,
                        };
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.label(RichText::new(&f.title).strong().color(colour));
                            ui.label(RichText::new(&f.detail).color(theme::DIM));
                            if let Some(fix) = &f.fix {
                                if ui
                                    .button(&fix.label)
                                    .on_hover_text(&fix.command)
                                    .clicked()
                                {
                                    fix_cmd = Some(fix.command.clone());
                                }
                            }
                        });
                    }
                    for n in &rep.notes {
                        ui.label(RichText::new(n).color(theme::DIM));
                    }
                    ui.separator();

                    let (problems, working): (Vec<&Device>, Vec<&Device>) =
                        rep.devices.iter().partition(|d| d.state != State::Ok);
                    if !problems.is_empty() {
                        ui.label(RichText::new("Devices needing attention").strong());
                        for d in problems {
                            device_row(ui, d, &mut search);
                        }
                    }
                    egui::CollapsingHeader::new(format!("Working devices ({})", working.len()))
                        .id_source("hw_working")
                        .default_open(false)
                        .show(ui, |ui| {
                            for d in working {
                                ui.label(format!("{}  ·  {}", d.name, d.class));
                            }
                        });
                });
        });
        self.win.hardware = open;

        if rescan {
            self.start_hardware_scan();
        }
        if let Some(cmd) = fix_cmd {
            // Opens the usual "Run this command?" dialog; nothing runs until approved.
            self.pending_run = Some(cmd);
        }
        if let Some(url) = search {
            if let Err(e) = exec::open(&url) {
                self.toast = Some((format!("Could not open the browser: {e}"), Instant::now()));
            }
        }
    }
}

fn device_row(ui: &mut egui::Ui, d: &Device, search: &mut Option<String>) {
    ui.horizontal(|ui| {
        let (label, colour) = match &d.state {
            State::Missing => ("No driver", theme::BAD),
            State::Disabled => ("Switched off", theme::WARN),
            State::Problem(p) => (p.as_str(), theme::BAD),
            State::Ok => ("OK", theme::OK),
        };
        ui.colored_label(colour, label);
        ui.label(RichText::new(&d.name).strong());
        if ui
            .small_button("Find driver")
            .on_hover_text("Search the web for this device's driver")
            .clicked()
        {
            *search = Some(hardware::driver_search_url(d));
        }
    });
}
