//! First-run welcome: connect Claude, install the free tools, pick a look,
//! and run a first scan, in about a minute.

use super::*;
use orbit_core::{antivirus, store, BackendKind};

#[derive(Default)]
pub(super) struct WelcomeState {
    pub open: bool,
    pub step: usize,
}

const STEPS: [&str; 6] = [
    "Welcome",
    "Connect Claude",
    "Free tools",
    "Your look",
    "Protection",
    "Ready",
];
const TOOLS: [&str; 6] = [
    "Claude Code",
    "ClamAV",
    "mpv",
    "LibreOffice",
    "YARA",
    "whisper.cpp",
];

impl Desktop {
    pub(super) fn welcome_ui(&mut self, ctx: &egui::Context) {
        if !self.welcome.open {
            return;
        }
        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new("welcome_dim"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                ui.allocate_rect(screen, egui::Sense::click());
                ui.painter()
                    .rect_filled(screen, 0.0, Color32::from_black_alpha(140));
            });
        let mut finish = false;
        let mut install: Option<store::StoreApp> = None;
        egui::Area::new(egui::Id::new("welcome_card"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| egui::Frame::window(ui.style()).inner_margin(egui::Margin::same(16.0)).show(ui, |ui| {
                ui.set_width(620.0);
                // Progress dots.
                ui.horizontal(|ui| {
                    for (i, s) in STEPS.iter().enumerate() {
                        let t = RichText::new(*s).small();
                        ui.label(if i == self.welcome.step { t.strong().color(theme::accent()) } else if i < self.welcome.step { t.color(theme::OK) } else { t.color(theme::DIM) });
                        if i + 1 < STEPS.len() {
                            ui.label(RichText::new("›").color(theme::DIM));
                        }
                    }
                });
                ui.separator();
                ui.vertical(|ui| {
                ui.set_min_height(330.0);
                match self.welcome.step {
                    0 => {
                        ui.add_space(20.0);
                        ui.vertical_centered(|ui| {
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(80.0, 80.0), egui::Sense::hover());
                            theme::paint_icon(ui.painter(), rect, theme::Glyph::Claude, "");
                            ui.label(RichText::new("Welcome to OrbitOS").size(30.0).strong());
                            ui.label(format!("A Rust desktop with Claude built in, running on {}.", orbit_core::host_os()));
                            ui.add_space(10.0);
                            ui.label("In the next minute we'll connect Claude, add a few free tools,\npick your look, and check your computer is clean.");
                        });
                    }
                    1 => {
                        ui.label(RichText::new("How should OrbitOS reach Claude?").size(18.0).strong());
                        let has_cli = orbit_core::exec::which("claude").is_some();
                        ui.radio_value(&mut self.cfg.backend, BackendKind::Cli, format!(
                            "Claude Code CLI: no key stored {}",
                            if has_cli { "(found on this computer ✓)" } else { "(not installed yet)" }
                        ));
                        if !has_cli && self.cfg.backend == BackendKind::Cli {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Needs Node.js, then:").small().color(theme::DIM));
                                ui.label(RichText::new("npm install -g @anthropic-ai/claude-code").monospace().small());
                                if ui.small_button("Install").clicked() {
                                    install = store::catalog().into_iter().find(|a| a.name == "Claude Code");
                                }
                            });
                            ui.label(RichText::new("Then run `claude` once in Terminal to sign in.").small().color(theme::DIM));
                        }
                        ui.radio_value(&mut self.cfg.backend, BackendKind::Api, "Anthropic API key");
                        if self.cfg.backend == BackendKind::Api {
                            ui.add(egui::TextEdit::singleline(&mut self.key_edit).password(true).hint_text("sk-ant-…"));
                        }
                        ui.radio_value(&mut self.cfg.backend, BackendKind::Local, "My own AI server (vLLM, Ollama, LM Studio)");
                        if self.cfg.backend == BackendKind::Local {
                            ui.horizontal(|ui| {
                                ui.label("Server URL");
                                ui.text_edit_singleline(&mut self.cfg.local_url);
                            });
                        }
                        ui.add_space(8.0);
                        if ui.button("Test connection").clicked() {
                            if self.cfg.backend == BackendKind::Api {
                                self.cfg.api_key = Some(self.key_edit.trim().to_string()).filter(|k| !k.is_empty());
                            }
                            self.send_to_claude("Reply with one short, friendly sentence welcoming me to OrbitOS.".into());
                            self.win.claude = true;
                        }
                        ui.label(RichText::new("The reply appears in the Claude window. You can change this any time in Settings.").small().color(theme::DIM));
                    }
                    2 => {
                        ui.label(RichText::new("Free tools that make OrbitOS shine").size(18.0).strong());
                        ui.label(RichText::new("All open source. OrbitOS works without them, but each one unlocks a feature.").color(theme::DIM));
                        ui.add_space(6.0);
                        let catalog = store::catalog();
                        egui::Grid::new("welcome_tools").num_columns(3).spacing([12.0, 8.0]).show(ui, |ui| {
                            for name in TOOLS {
                                let Some(a) = catalog.iter().find(|a| a.name == name) else { continue };
                                ui.label(RichText::new(a.name).strong());
                                ui.label(RichText::new(a.about).small());
                                if self.store.installed.contains(a.name) {
                                    ui.label(RichText::new("Installed").color(theme::OK));
                                } else if store::install_command(a).is_some() {
                                    if ui.button("Install").clicked() {
                                        install = Some(a.clone());
                                    }
                                } else if ui.link("Get it").clicked() {
                                    let _ = exec::open(a.homepage);
                                }
                                ui.end_row();
                            }
                        });
                        if ui.small_button("Re-check").clicked() {
                            self.refresh_store();
                        }
                    }
                    3 => {
                        ui.label(RichText::new("Pick your look").size(18.0).strong());
                        ui.horizontal_wrapped(|ui| {
                            for t in theme::THEMES {
                                let (rect, resp) = ui.allocate_exact_size(egui::vec2(104.0, 70.0), egui::Sense::click());
                                let p = ui.painter();
                                p.rect_filled(rect, egui::Rounding::same(10.0), Color32::from_rgb(t.sky[0], t.sky[1], t.sky[2]));
                                p.circle_filled(rect.center() - egui::vec2(20.0, 6.0), 16.0, Color32::from_rgb(t.glow1[0], t.glow1[1], t.glow1[2]));
                                p.circle_filled(rect.center() + egui::vec2(22.0, 8.0), 12.0, Color32::from_rgb(t.glow2[0], t.glow2[1], t.glow2[2]));
                                p.text(rect.center_bottom() - egui::vec2(0.0, 10.0), egui::Align2::CENTER_CENTER, t.name, egui::FontId::proportional(13.0), if t.dark { Color32::WHITE } else { Color32::BLACK });
                                if self.cfg.theme == t.key {
                                    p.rect_stroke(rect, egui::Rounding::same(10.0), egui::Stroke::new(3.0_f32, theme::accent()));
                                }
                                if resp.clicked() {
                                    self.cfg.theme = t.key.into();
                                    self.cfg.accent = None;
                                    theme::apply(ctx, &self.cfg);
                                }
                            }
                        });
                        ui.add_space(8.0);
                        ui.label(RichText::new("Wallpaper").strong());
                        ui.horizontal_wrapped(|ui| {
                            for (k, n) in theme::WALLPAPERS.iter().filter(|(k, _)| *k != "image") {
                                ui.selectable_value(&mut self.cfg.wallpaper, k.to_string(), *n);
                            }
                        });
                        ui.checkbox(&mut self.cfg.animate_wallpaper, "Animate the wallpaper");
                    }
                    4 => {
                        ui.label(RichText::new("Orbit Shield").size(18.0).strong());
                        ui.checkbox(&mut self.cfg.shield_watch, "Check new downloads automatically (real-time watch)");
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut self.cfg.scheduled_scan, "Daily quick scan at");
                            ui.add(egui::Slider::new(&mut self.cfg.scan_hour, 0..=23).suffix(":00"));
                        });
                        ui.add_space(8.0);
                        if self.shield.scanning {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(format!("Scanning… {} files checked", self.shield.count));
                            });
                        } else if let Some(s) = &self.shield.summary {
                            let clean = self.shield.findings.is_empty();
                            ui.colored_label(if clean { theme::OK } else { theme::WARN }, s);
                            if !clean {
                                ui.label("Open Orbit Shield after this tour to review what was found.");
                            }
                        } else if ui.button(RichText::new("Run a quick scan now").strong()).clicked() {
                            self.start_scan(antivirus::quick_scan_paths());
                        }
                    }
                    _ => {
                        ui.add_space(10.0);
                        ui.label(RichText::new("You're all set").size(26.0).strong());
                        ui.add_space(6.0);
                        for tip in [
                            "Ctrl+Space (Cmd+Space on Mac) opens the command bar: apps, files, maths, Claude",
                            "Ask Claude to do things. Every action asks for your approval first",
                            "Click Mic (or hold F9) to talk; enable \"Hey Orbit\" in Settings -> Voice",
                            "Ctrl+Alt+← / -> snaps windows; Ctrl+Alt+1-4 switches desktops",
                            "The bell on the top bar opens notifications and widgets",
                            "The App Store installs free apps in one click",
                        ] {
                            ui.label(format!("•  {tip}"));
                        }
                    }
                }
                });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Skip tour").clicked() {
                        finish = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let last = self.welcome.step + 1 == STEPS.len();
                        if ui.button(RichText::new(if last { "Start using OrbitOS" } else { "Next" }).strong()).clicked() {
                            if last {
                                finish = true;
                            } else {
                                self.welcome.step += 1;
                                if self.welcome.step == 2 {
                                    self.refresh_store();
                                }
                            }
                        }
                        if self.welcome.step > 0 && ui.button("Back").clicked() {
                            self.welcome.step -= 1;
                        }
                    });
                });
            }));
        if let Some(a) = install {
            self.request_install(&a);
        }
        if finish {
            self.welcome.open = false;
            self.welcome.step = 0;
            self.cfg.onboarded = true;
            if self.cfg.backend == BackendKind::Api && !self.key_edit.trim().is_empty() {
                self.cfg.api_key = Some(self.key_edit.trim().to_string());
            }
            let _ = self.cfg.save();
            self.push_note(
                "Welcome to OrbitOS",
                "Press Ctrl+Space any time to search or ask Claude",
            );
        }
    }
}
