//! Notification centre and widgets: clock, calendar, weather, now playing,
//! system and Orbit Shield status, and every notification.

use super::icons::WinId;
use super::*;
use chrono::{Datelike, Local, NaiveDate};
use orbit_core::weather::Weather;

#[derive(Clone)]
pub(super) struct Note {
    pub time: String,
    pub title: String,
    pub body: String,
}

#[derive(Default)]
pub(super) struct CenterState {
    pub open: bool,
    pub notes: Vec<Note>,
    pub unread: usize,
    pub weather: Option<Weather>,
    pub weather_err: Option<String>,
    pub weather_at: Option<Instant>,
    pub weather_loading: bool,
    pub month_offset: i32,
}

impl Desktop {
    /// Add to the notification centre and show a toast.
    pub(super) fn push_note(&mut self, title: &str, body: &str) {
        self.center.notes.insert(
            0,
            Note {
                time: Local::now().format("%H:%M").to_string(),
                title: title.to_string(),
                body: body.to_string(),
            },
        );
        self.center.notes.truncate(100);
        if !self.center.open {
            self.center.unread += 1;
        }
        let toast = if body.is_empty() {
            title.to_string()
        } else {
            format!("{title}: {body}")
        };
        self.toast = Some((toast, Instant::now()));
    }

    pub(super) fn weather_tick(&mut self) {
        let stale = self
            .center
            .weather_at
            .map(|t| t.elapsed() > Duration::from_secs(30 * 60))
            .unwrap_or(true);
        if stale && !self.center.weather_loading && !self.cfg.weather_city.trim().is_empty() {
            self.center.weather_loading = true;
            self.center.weather_at = Some(Instant::now());
            let city = self.cfg.weather_city.clone();
            let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
            std::thread::spawn(move || {
                let _ = tx.send(Event::Weather(
                    orbit_core::weather::fetch(&city).map_err(|e| e.to_string()),
                ));
                ctx.request_repaint();
            });
        }
    }

    pub(super) fn center_panel(&mut self, ctx: &egui::Context) {
        if !self.center.open {
            return;
        }
        self.center.unread = 0;
        egui::SidePanel::right("center")
            .exact_width(320.0)
            .resizable(false)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        ui.set_max_width(300.0);
                        ui.add_space(8.0);
                        // Clock.
                        let now = Local::now();
                        ui.label(
                            RichText::new(now.format("%H:%M").to_string())
                                .size(46.0)
                                .strong(),
                        );
                        ui.label(
                            RichText::new(now.format("%A %d %B %Y").to_string()).color(theme::DIM),
                        );
                        ui.add_space(6.0);

                        widget(ui, "Weather", |ui| self.weather_widget(ui));
                        widget(ui, "Calendar", |ui| self.calendar_widget(ui));
                        widget(ui, "Now playing", |ui| self.now_playing_widget(ui));
                        widget(ui, "System", |ui| {
                            let s = &self.snap;
                            ui.add(
                                egui::ProgressBar::new(s.cpu_total / 100.0)
                                    .desired_width(270.0)
                                    .text(format!("CPU {:.0}%", s.cpu_total))
                                    .fill(theme::load_colour(s.cpu_total / 100.0)),
                            );
                            ui.add(
                                egui::ProgressBar::new(s.mem_fraction())
                                    .desired_width(270.0)
                                    .text(format!(
                                        "Memory {} / {}",
                                        human_bytes(s.mem_used),
                                        human_bytes(s.mem_total)
                                    ))
                                    .fill(theme::load_colour(s.mem_fraction())),
                            );
                            ui.label(
                                RichText::new(format!("Up {}", s.uptime_text()))
                                    .small()
                                    .color(theme::DIM),
                            );
                        });
                        let threats = self.shield.findings.len();
                        let mut open_shield = false;
                        widget(ui, "Orbit Shield", |ui| {
                            if threats > 0 {
                                ui.colored_label(
                                    theme::BAD,
                                    format!("{threats} item(s) to review"),
                                );
                            } else {
                                ui.colored_label(theme::SHIELD_GREEN, "Protected");
                            }
                            ui.label(
                                RichText::new(if self.cfg.shield_watch {
                                    "Real-time watch on"
                                } else {
                                    "Real-time watch off"
                                })
                                .small()
                                .color(theme::DIM),
                            );
                            if ui.small_button("Open Shield").clicked() {
                                open_shield = true;
                            }
                        });
                        if open_shield {
                            self.open_win(WinId::Shield);
                        }

                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Notifications").strong());
                            if !self.center.notes.is_empty()
                                && ui.small_button("Clear all").clicked()
                            {
                                self.center.notes.clear();
                            }
                        });
                        if self.center.notes.is_empty() {
                            ui.label(RichText::new("You're all caught up.").color(theme::DIM));
                        }
                        let mut remove = None;
                        for (i, n) in self.center.notes.iter().enumerate() {
                            egui::Frame::group(ui.style()).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(&n.title).strong());
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui.small_button("✕").clicked() {
                                                remove = Some(i);
                                            }
                                            ui.label(
                                                RichText::new(&n.time).small().color(theme::DIM),
                                            );
                                        },
                                    );
                                });
                                if !n.body.is_empty() {
                                    ui.add(
                                        egui::Label::new(RichText::new(&n.body).small()).wrap(true),
                                    );
                                }
                            });
                        }
                        if let Some(i) = remove {
                            self.center.notes.remove(i);
                        }
                    });
            });
    }

    fn weather_widget(&mut self, ui: &mut egui::Ui) {
        match (&self.center.weather, &self.center.weather_err) {
            (Some(w), _) => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{:.0}°", w.temp)).size(30.0).strong());
                    ui.vertical(|ui| {
                        ui.label(w.desc);
                        ui.label(
                            RichText::new(format!("{} · wind {:.0} km/h", w.place, w.wind))
                                .small()
                                .color(theme::DIM),
                        );
                    });
                });
                ui.horizontal(|ui| {
                    for d in w.days.iter().skip(1).take(3) {
                        let day = NaiveDate::parse_from_str(&d.date, "%Y-%m-%d")
                            .map(|n| n.format("%a").to_string())
                            .unwrap_or_default();
                        ui.vertical(|ui| {
                            ui.label(RichText::new(day).small().strong());
                            ui.label(
                                RichText::new(format!("{:.0}° / {:.0}°", d.max, d.min)).small(),
                            );
                            ui.label(RichText::new(d.desc).small().color(theme::DIM));
                        });
                        ui.add_space(8.0);
                    }
                });
            }
            (None, Some(e)) => {
                ui.label(
                    RichText::new("Weather unavailable right now (offline?)")
                        .small()
                        .color(theme::DIM),
                )
                .on_hover_text(e.as_str());
                if ui.small_button("Retry").clicked() {
                    self.center.weather_at = None;
                }
            }
            _ => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Loading weather…");
                });
            }
        }
    }

    fn calendar_widget(&mut self, ui: &mut egui::Ui) {
        let today = Local::now().date_naive();
        let mut y = today.year();
        let mut m = today.month() as i32 + self.center.month_offset;
        while m < 1 {
            m += 12;
            y -= 1;
        }
        while m > 12 {
            m -= 12;
            y += 1;
        }
        let first = NaiveDate::from_ymd_opt(y, m as u32, 1).unwrap();
        ui.horizontal(|ui| {
            if ui.small_button("◀").clicked() {
                self.center.month_offset -= 1;
            }
            ui.label(RichText::new(first.format("%B %Y").to_string()).strong());
            if ui.small_button("▶").clicked() {
                self.center.month_offset += 1;
            }
            if self.center.month_offset != 0 && ui.small_button("Today").clicked() {
                self.center.month_offset = 0;
            }
        });
        let days_in = {
            let (ny, nm) = if m == 12 {
                (y + 1, 1)
            } else {
                (y, m as u32 + 1)
            };
            NaiveDate::from_ymd_opt(ny, nm, 1)
                .unwrap()
                .signed_duration_since(first)
                .num_days() as u32
        };
        let lead = first.weekday().num_days_from_monday();
        egui::Grid::new("cal")
            .spacing([4.0, 2.0])
            .min_col_width(32.0)
            .show(ui, |ui| {
                for d in ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"] {
                    ui.label(RichText::new(d).small().color(theme::DIM));
                }
                ui.end_row();
                let mut col = 0;
                for _ in 0..lead {
                    ui.label("");
                    col += 1;
                }
                for day in 1..=days_in {
                    let is_today = NaiveDate::from_ymd_opt(y, m as u32, day) == Some(today);
                    let t = RichText::new(format!("{day:>2}")).monospace();
                    ui.label(if is_today {
                        t.strong().color(theme::accent())
                    } else {
                        t
                    });
                    col += 1;
                    if col % 7 == 0 {
                        ui.end_row();
                    }
                }
            });
    }

    fn now_playing_widget(&mut self, ui: &mut egui::Ui) {
        if self.media.stream.is_playing() {
            ui.label(
                RichText::new(&self.media.stream.title)
                    .strong()
                    .color(theme::MEDIA_PINK),
            );
            if ui.small_button("Stop").clicked() {
                self.media.stream.stop();
            }
            return;
        }
        #[cfg(feature = "audio")]
        if let Some(a) = self.media.audio.as_mut() {
            if let Some(cur) = a.current().cloned() {
                let title = cur
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                ui.label(RichText::new(title).strong().color(theme::MEDIA_PINK));
                ui.horizontal(|ui| {
                    if ui.small_button("⏮").clicked() {
                        let _ = a.prev();
                    }
                    if ui
                        .small_button(if a.is_paused() { "▶" } else { "⏸" })
                        .clicked()
                    {
                        a.toggle();
                    }
                    if ui.small_button("⏭").clicked() {
                        let _ = a.next();
                    }
                });
                return;
            }
        }
        ui.label(RichText::new("Nothing playing").color(theme::DIM));
        if ui.small_button("Open Media").clicked() {
            self.open_win(WinId::Media);
        }
    }
}

fn widget(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::group(ui.style())
        .rounding(egui::Rounding::same(10.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width().min(286.0));
            ui.set_max_width(286.0);
            ui.label(RichText::new(title).small().strong().color(theme::accent()));
            body(ui);
        });
    ui.add_space(4.0);
}
