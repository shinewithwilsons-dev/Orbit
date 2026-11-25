//! Settings: Claude & AI, Appearance, Voice, Media, Shield, Backup, General.

use super::icons::WinId;
use super::*;
use orbit_core::{antivirus, backup, voice, BackendKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Tab {
    #[default]
    Claude,
    Appearance,
    Voice,
    Media,
    Shield,
    Backup,
    General,
}

#[derive(Default)]
pub(super) struct SettingsState {
    pub local_models: Vec<String>,
    pub local_status: Option<String>,
    pub backup_password: String,
    pub restore_password: String,
    pub backup_status: Option<String>,
    pub confirm_restore: Option<PathBuf>,
    pub image_path: String,
    pub autostart: Option<bool>,
}

impl Desktop {
    fn backup_dir(&self) -> PathBuf {
        self.cfg
            .backup_dir
            .as_ref()
            .filter(|d| !d.trim().is_empty())
            .map(|d| orbit_core::expand_tilde(d))
            .unwrap_or_else(|| {
                backup::suggested_dirs()
                    .last()
                    .map(|(_, p)| p.clone())
                    .unwrap_or_else(|| ".".into())
            })
    }

    pub(super) fn run_backup(&mut self, password: String, quiet: bool) {
        let dir = self.backup_dir();
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        let _ = self.cfg.save();
        self.save_chat();
        self.wm.save();
        std::thread::spawn(move || {
            let r = backup::create(&dir, &password)
                .map(|p| p.display().to_string())
                .map_err(|e| e.to_string());
            let _ = tx.send(Event::BackupDone(r, quiet));
            ctx.request_repaint();
        });
    }

    /// Daily automatic backup (secrets removed, since it's unencrypted).
    pub(super) fn backup_tick(&mut self) {
        if self.cfg.auto_backup
            && self.cfg.backup_dir.is_some()
            && orbit_core::today() > self.cfg.last_backup_day
        {
            self.cfg.last_backup_day = orbit_core::today();
            let _ = self.cfg.save();
            self.run_backup(String::new(), true);
        }
    }

    pub(super) fn load_wallpaper_image(&mut self) {
        self.wallpaper_tex = None;
        let Some(p) = self.cfg.wallpaper_image.clone() else {
            return;
        };
        match image::open(orbit_core::expand_tilde(&p)) {
            Ok(img) => {
                let img = img.thumbnail(2560, 1600).to_rgba8();
                let size = [img.width() as usize, img.height() as usize];
                let ci = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
                self.wallpaper_tex = Some(self.ctx.load_texture(
                    "wallpaper",
                    ci,
                    egui::TextureOptions::LINEAR,
                ));
            }
            Err(e) => self.push_note("Wallpaper", &format!("Couldn't load picture: {e}")),
        }
    }

    pub(super) fn settings_window(&mut self, ctx: &egui::Context) {
        let mut open = self.win.settings;
        if !self.wm_visible(WinId::Settings) {
            return;
        }
        let mut save = false;
        let mut reapply = false;
        self.wm_window(WinId::Settings, "Settings", [360.0, 80.0, 620.0, 500.0])
            .open(&mut open)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (t, n) in [
                        (Tab::Claude, "Claude & AI"),
                        (Tab::Appearance, "Appearance"),
                        (Tab::Voice, "Voice"),
                        (Tab::Media, "Media"),
                        (Tab::Shield, "Shield"),
                        (Tab::Backup, "Backup & sync"),
                        (Tab::General, "General"),
                    ] {
                        ui.selectable_value(&mut self.settings_tab, t, n);
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .max_height(ui.available_height() - 40.0)
                    .show(ui, |ui| match self.settings_tab {
                        Tab::Claude => self.settings_claude(ui),
                        Tab::Appearance => reapply |= self.settings_appearance(ui),
                        Tab::Voice => self.settings_voice(ui),
                        Tab::Media => self.settings_media(ui),
                        Tab::Shield => self.settings_shield(ui),
                        Tab::Backup => self.settings_backup(ui),
                        Tab::General => self.settings_general(ui),
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("Save").strong()).clicked() {
                        save = true;
                    }
                    ui.label(
                        RichText::new(format!("Settings file: {}", Config::path().display()))
                            .small()
                            .color(theme::DIM),
                    );
                });
            });
        self.win.settings = open;
        if reapply {
            theme::apply(ctx, &self.cfg);
        }
        if save {
            self.cfg.api_key = if self.key_edit.trim().is_empty() {
                None
            } else {
                Some(self.key_edit.trim().to_string())
            };
            match self.cfg.save() {
                Ok(()) => self.notify("Settings saved"),
                Err(e) => self.notify(format!("Save failed: {e}")),
            }
        }
        self.confirm_restore_dialog(ctx);
    }

    fn settings_claude(&mut self, ui: &mut egui::Ui) {
        ui.heading("How OrbitOS reaches Claude");
        ui.radio_value(
            &mut self.cfg.backend,
            BackendKind::Cli,
            "Claude Code CLI: your signed-in `claude`, no key stored",
        );
        ui.radio_value(
            &mut self.cfg.backend,
            BackendKind::Api,
            "Anthropic API: uses an API key",
        );
        ui.radio_value(
            &mut self.cfg.backend,
            BackendKind::Local,
            "Local server: your vLLM (or Ollama / LM Studio / llama.cpp)",
        );
        ui.radio_value(
            &mut self.cfg.backend,
            BackendKind::Auto,
            "Auto: API if a key is available, otherwise CLI",
        );
        ui.label(
            RichText::new(format!("Now using: {}", claude::backend_status(&self.cfg)))
                .color(theme::accent()),
        );
        ui.add_space(6.0);
        ui.collapsing("Anthropic API", |ui| {
            egui::Grid::new("api").num_columns(2).show(ui, |ui| {
                ui.label("Model");
                ui.text_edit_singleline(&mut self.cfg.model);
                ui.end_row();
                ui.label("Max tokens");
                ui.add(egui::DragValue::new(&mut self.cfg.max_tokens).clamp_range(256..=64_000));
                ui.end_row();
                ui.label("API key");
                ui.add(
                    egui::TextEdit::singleline(&mut self.key_edit)
                        .password(true)
                        .hint_text("sk-ant-… (optional)"),
                );
                ui.end_row();
                ui.label("CLI path");
                let mut p = self.cfg.cli_path.clone().unwrap_or_default();
                if ui
                    .add(egui::TextEdit::singleline(&mut p).hint_text("auto-detect `claude`"))
                    .changed()
                {
                    self.cfg.cli_path = if p.trim().is_empty() { None } else { Some(p) };
                }
                ui.end_row();
            });
            if std::env::var("ANTHROPIC_API_KEY").is_ok() {
                ui.label(
                    RichText::new(
                        "ANTHROPIC_API_KEY is set in the environment and takes priority.",
                    )
                    .small()
                    .color(theme::DIM),
                );
            }
        });
        egui::CollapsingHeader::new("Local AI server (vLLM)").default_open(self.cfg.backend == BackendKind::Local).show(ui, |ui| {
            ui.label(RichText::new("Any OpenAI-compatible server. For vLLM: `vllm serve <model>` listens on http://localhost:8000/v1").small().color(theme::DIM));
            egui::Grid::new("local").num_columns(2).show(ui, |ui| {
                ui.label("Server URL");
                ui.text_edit_singleline(&mut self.cfg.local_url);
                ui.end_row();
                ui.label("Model");
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_source("local_model")
                        .selected_text(if self.cfg.local_model.is_empty() { "first available".to_string() } else { self.cfg.local_model.clone() })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.cfg.local_model, String::new(), "first available");
                            for m in &self.settings.local_models {
                                ui.selectable_value(&mut self.cfg.local_model, m.clone(), m);
                            }
                        });
                });
                ui.end_row();
                ui.label("API key");
                let mut k = self.cfg.local_api_key.clone().unwrap_or_default();
                if ui.add(egui::TextEdit::singleline(&mut k).password(true).hint_text("only if your server needs one")).changed() {
                    self.cfg.local_api_key = if k.trim().is_empty() { None } else { Some(k) };
                }
                ui.end_row();
                ui.label("Image model");
                ui.add(egui::TextEdit::singleline(&mut self.cfg.image_model).hint_text("optional, for /image"));
                ui.end_row();
            });
            ui.horizontal(|ui| {
                if ui.button("Test connection").clicked() {
                    self.settings.local_status = Some("Connecting…".into());
                    let cfg = self.cfg.clone();
                    let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
                    std::thread::spawn(move || {
                        let _ = tx.send(Event::LocalModels(claude::list_local_models(&cfg).map_err(|e| e.to_string())));
                        ctx.request_repaint();
                    });
                }
                if let Some(s) = &self.settings.local_status {
                    ui.label(RichText::new(s).small());
                }
            });
            ui.label(RichText::new("Images: type /image <description> in the Claude window, or ask Claude to generate one. Needs an OpenAI-style /v1/images/generations endpoint on your server.").small().color(theme::DIM));
        });
        ui.add_space(6.0);
        if ui.button("Test Claude").clicked() {
            self.send_to_claude(
                "Reply with one short sentence confirming you're connected to OrbitOS.".into(),
            );
        }
    }

    /// Returns true if the theme must be re-applied.
    fn settings_appearance(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        ui.heading("Theme");
        ui.horizontal_wrapped(|ui| {
            for t in theme::THEMES {
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(100.0, 64.0), egui::Sense::click());
                let p = ui.painter();
                p.rect_filled(
                    rect,
                    egui::Rounding::same(10.0),
                    Color32::from_rgb(t.sky[0], t.sky[1], t.sky[2]),
                );
                p.circle_filled(
                    rect.center() - egui::vec2(18.0, 6.0),
                    14.0,
                    Color32::from_rgb(t.glow1[0], t.glow1[1], t.glow1[2]),
                );
                p.circle_filled(
                    rect.center() + egui::vec2(20.0, 6.0),
                    10.0,
                    Color32::from_rgb(t.glow2[0], t.glow2[1], t.glow2[2]),
                );
                p.text(
                    rect.center_bottom() - egui::vec2(0.0, 9.0),
                    egui::Align2::CENTER_CENTER,
                    t.name,
                    egui::FontId::proportional(12.5),
                    if t.dark {
                        Color32::WHITE
                    } else {
                        Color32::BLACK
                    },
                );
                if self.cfg.theme == t.key {
                    p.rect_stroke(
                        rect,
                        egui::Rounding::same(10.0),
                        egui::Stroke::new(3.0_f32, theme::accent()),
                    );
                }
                if resp.clicked() {
                    self.cfg.theme = t.key.into();
                    changed = true;
                }
            }
        });
        ui.horizontal(|ui| {
            let mut custom = self.cfg.accent.is_some();
            if ui.checkbox(&mut custom, "Custom accent colour").changed() {
                self.cfg.accent = if custom {
                    Some(theme::theme_def(&self.cfg.theme).accent)
                } else {
                    None
                };
                changed = true;
            }
            if let Some(a) = self.cfg.accent.as_mut() {
                if egui::color_picker::color_edit_button_srgb(ui, a).changed() {
                    changed = true;
                }
            }
        });
        ui.add_space(6.0);
        ui.heading("Display");
        ui.horizontal(|ui| {
            ui.label("Interface size");
            let label = |v: Option<f32>| match v {
                None => "Auto (fits the screen)".to_string(),
                Some(z) => format!("{:.0}%", z * 100.0),
            };
            egui::ComboBox::from_id_source("ui_scale")
                .selected_text(label(self.cfg.ui_scale))
                .show_ui(ui, |ui| {
                    for v in [
                        None,
                        Some(1.0),
                        Some(1.25),
                        Some(1.5),
                        Some(1.75),
                        Some(2.0),
                        Some(2.5),
                    ] {
                        if ui
                            .selectable_value(&mut self.cfg.ui_scale, v, label(v))
                            .changed()
                        {
                            changed = true;
                            self.apply_ui_scale();
                        }
                    }
                });
        });
        ui.label(
            RichText::new(
                "Auto makes text and buttons larger on 4K screens. Ctrl + / Ctrl − also zoom.",
            )
            .small()
            .color(theme::DIM),
        );
        ui.add_space(6.0);
        ui.heading("Wallpaper");
        ui.horizontal_wrapped(|ui| {
            for (k, n) in theme::WALLPAPERS {
                ui.selectable_value(&mut self.cfg.wallpaper, k.to_string(), *n);
            }
        });
        if self.cfg.wallpaper == "image" {
            ui.horizontal(|ui| {
                if self.settings.image_path.is_empty() {
                    self.settings.image_path = self.cfg.wallpaper_image.clone().unwrap_or_default();
                }
                ui.add(
                    egui::TextEdit::singleline(&mut self.settings.image_path)
                        .hint_text("path to a .jpg or .png")
                        .desired_width(320.0),
                );
                if ui.button("Use picture").clicked() {
                    self.cfg.wallpaper_image = Some(self.settings.image_path.trim().to_string());
                    self.load_wallpaper_image();
                }
            });
            ui.label(
                RichText::new("Tip: right-click an image in Files and copy its path.")
                    .small()
                    .color(theme::DIM),
            );
        }
        ui.checkbox(
            &mut self.cfg.animate_wallpaper,
            "Animate wallpaper (uses a little more power)",
        );
        changed
    }

    fn settings_voice(&mut self, ui: &mut egui::Ui) {
        ui.heading("Voice");
        match self.voice_ready() {
            Ok(()) => {
                ui.colored_label(
                    theme::OK,
                    "Ready: speech is recognised offline by whisper.cpp",
                );
            }
            Err(e) => {
                ui.colored_label(theme::WARN, "Not set up yet");
                ui.label(RichText::new(e).small());
            }
        }
        egui::Grid::new("voice").num_columns(2).show(ui, |ui| {
            ui.label("whisper.cpp program");
            let mut b = self.cfg.whisper_bin.clone().unwrap_or_default();
            let found = voice::find_whisper(&self.cfg)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "not found".into());
            if ui
                .add(egui::TextEdit::singleline(&mut b).hint_text(found))
                .changed()
            {
                self.cfg.whisper_bin = if b.trim().is_empty() { None } else { Some(b) };
            }
            ui.end_row();
            ui.label("Model file");
            let mut m = self.cfg.whisper_model.clone().unwrap_or_default();
            let found = voice::find_model(&self.cfg)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "not found".into());
            if ui
                .add(egui::TextEdit::singleline(&mut m).hint_text(found))
                .changed()
            {
                self.cfg.whisper_model = if m.trim().is_empty() { None } else { Some(m) };
            }
            ui.end_row();
        });
        if ui.small_button("Open models folder").clicked() {
            let d = voice::models_dir();
            let _ = std::fs::create_dir_all(&d);
            let _ = exec::open(&d.to_string_lossy());
        }
        ui.separator();
        ui.checkbox(
            &mut self.cfg.wake_word,
            "Listen for \"Hey Orbit\" (uses the microphone continuously)",
        );
        ui.checkbox(&mut self.cfg.speak_replies, "Speak Claude's replies aloud");
        ui.label(
            RichText::new("Push-to-talk: click Mic on the top bar, or hold F9.")
                .small()
                .color(theme::DIM),
        );
        if ui.button("Test speech").clicked() {
            self.speak("Hello, I'm Orbit. Voice replies are working.");
        }
    }

    fn settings_media(&mut self, ui: &mut egui::Ui) {
        ui.heading("Media");
        ui.horizontal(|ui| {
            ui.label("Volume");
            ui.add(egui::Slider::new(&mut self.cfg.volume, 0.0..=1.0).show_value(false));
        });
        ui.label("Equaliser (also in the Media window)");
        for (i, n) in ["Bass", "Mid", "Treble"].iter().enumerate() {
            ui.horizontal(|ui| {
                ui.add_sized([50.0, 18.0], egui::Label::new(*n));
                ui.add(egui::Slider::new(&mut self.cfg.eq[i], -12.0..=12.0).suffix(" dB"));
            });
        }
        ui.separator();
        ui.heading("BeMusic");
        ui.label(RichText::new("Stream from your own BeMusic site in the Media window. Create an API token in your BeMusic account settings.").small().color(theme::DIM));
        egui::Grid::new("bemusic").num_columns(2).show(ui, |ui| {
            ui.label("Site URL");
            let mut u = self.cfg.bemusic_url.clone().unwrap_or_default();
            if ui
                .add(egui::TextEdit::singleline(&mut u).hint_text("https://music.example.com"))
                .changed()
            {
                self.cfg.bemusic_url = if u.trim().is_empty() { None } else { Some(u) };
            }
            ui.end_row();
            ui.label("API token");
            let mut t = self.cfg.bemusic_token.clone().unwrap_or_default();
            if ui
                .add(egui::TextEdit::singleline(&mut t).password(true))
                .changed()
            {
                self.cfg.bemusic_token = if t.trim().is_empty() { None } else { Some(t) };
            }
            ui.end_row();
        });
    }

    fn settings_shield(&mut self, ui: &mut egui::Ui) {
        ui.heading("Orbit Shield");
        ui.checkbox(
            &mut self.cfg.shield_watch,
            "Real-time watch: check new files in Downloads and Desktop as they arrive",
        );
        ui.checkbox(&mut self.cfg.use_clamav, "Use ClamAV when installed");
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.cfg.scheduled_scan, "Daily quick scan at");
            ui.add(egui::Slider::new(&mut self.cfg.scan_hour, 0..=23).suffix(":00"));
        });
        ui.separator();
        ui.label(RichText::new("YARA rules").strong());
        match antivirus::find_yara() {
            Some(p) => ui.label(format!("YARA found: {}", p.display())),
            None => ui.label(
                RichText::new(
                    "YARA not installed. Get it from the App Store to use pattern rules.",
                )
                .color(theme::DIM),
            ),
        };
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(antivirus::yara_rules_dir().display().to_string())
                    .monospace()
                    .small(),
            );
            if ui.small_button("Open rules folder").clicked() {
                let _ = antivirus::yara_rule_files();
                let _ = exec::open(&antivirus::yara_rules_dir().to_string_lossy());
            }
        });
        ui.label(
            RichText::new(format!(
                "Your own SHA-256 signatures: {}",
                antivirus::signatures_path().display()
            ))
            .small()
            .color(theme::DIM),
        );
    }

    fn settings_backup(&mut self, ui: &mut egui::Ui) {
        ui.heading("Backup & sync");
        ui.label(RichText::new("Saves settings, Claude chats, playlists, window layout, Shield signatures and quarantine records. Put backups in a cloud folder (OneDrive, iCloud, Dropbox, Google Drive) to sync between computers.").small().color(theme::DIM));
        ui.horizontal(|ui| {
            ui.label("Folder");
            let mut d = self
                .cfg
                .backup_dir
                .clone()
                .unwrap_or_else(|| self.backup_dir().display().to_string());
            if ui
                .add(egui::TextEdit::singleline(&mut d).desired_width(360.0))
                .changed()
            {
                self.cfg.backup_dir = Some(d);
            }
        });
        ui.horizontal_wrapped(|ui| {
            for (name, p) in backup::suggested_dirs() {
                if ui
                    .small_button(name)
                    .on_hover_text(p.display().to_string())
                    .clicked()
                {
                    self.cfg.backup_dir = Some(p.display().to_string());
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("Password");
            ui.add(
                egui::TextEdit::singleline(&mut self.settings.backup_password)
                    .password(true)
                    .hint_text("recommended: encrypts the backup"),
            );
            if ui.button(RichText::new("Back up now").strong()).clicked() {
                let pw = self.settings.backup_password.clone();
                self.settings.backup_status = Some("Backing up…".into());
                self.run_backup(pw, false);
            }
        });
        ui.label(
            RichText::new("Without a password, API keys and tokens are left out of the backup.")
                .small()
                .color(theme::DIM),
        );
        ui.checkbox(
            &mut self.cfg.auto_backup,
            "Back up automatically once a day (without secrets)",
        );
        if let Some(s) = &self.settings.backup_status {
            ui.label(RichText::new(s).small());
        }
        ui.separator();
        ui.label(RichText::new("Restore").strong());
        ui.horizontal(|ui| {
            ui.label("Password");
            ui.add(
                egui::TextEdit::singleline(&mut self.settings.restore_password)
                    .password(true)
                    .hint_text("if the backup is encrypted"),
            );
        });
        let list = backup::list(&self.backup_dir());
        if list.is_empty() {
            ui.label(RichText::new("No backups in this folder yet.").color(theme::DIM));
        }
        for p in list.iter().take(10) {
            ui.horizontal(|ui| {
                ui.label(p.file_name().unwrap_or_default().to_string_lossy());
                if ui.small_button("Restore").clicked() {
                    self.settings.confirm_restore = Some(p.clone());
                }
            });
        }
    }

    fn settings_general(&mut self, ui: &mut egui::Ui) {
        ui.heading("Window & tray");
        ui.label("When I close the Orbit window:");
        ui.radio_value(
            &mut self.cfg.close_to_tray,
            true,
            "Keep Orbit running in the tray (recommended)",
        );
        ui.radio_value(&mut self.cfg.close_to_tray, false, "Quit Orbit");
        match self.tray_known {
            Some(false) => {
                ui.label(RichText::new("No system tray was found on this desktop, so closing minimises Orbit to the taskbar instead.").small().color(theme::WARN));
            }
            Some(true) => {
                ui.label(RichText::new("The Orbit icon is in your system tray. Click it to show or hide Orbit; right-click for the menu.").small().color(theme::DIM));
            }
            None => {}
        }
        let mut autostart = self
            .settings
            .autostart
            .unwrap_or_else(orbit_core::autostart::is_enabled);
        if ui
            .checkbox(
                &mut autostart,
                "Start Orbit when I sign in (it opens hidden in the tray)",
            )
            .changed()
        {
            match orbit_core::autostart::set_enabled(autostart) {
                Ok(()) => self.notify(if autostart {
                    "Orbit will start when you sign in"
                } else {
                    "Orbit won't start automatically"
                }),
                Err(e) => self.notify(format!("Couldn't change start-up: {e}")),
            }
            self.settings.autostart = Some(orbit_core::autostart::is_enabled());
        } else if self.settings.autostart.is_none() {
            self.settings.autostart = Some(autostart);
        }
        if ui
            .checkbox(
                &mut self.cfg.global_hotkey,
                format!(
                    "Show or hide Orbit from any app with {}",
                    tray::hotkey_label()
                ),
            )
            .changed()
        {
            let on = self.cfg.global_hotkey;
            self.set_hotkey(on);
            let _ = self.cfg.save();
        }
        ui.checkbox(
            &mut self.cfg.listen_while_hidden,
            "Keep listening for \"Hey Orbit\" while hidden",
        );
        ui.label(
            RichText::new(
                "Quit Orbit from the tray menu, the Orbit menu, the Start menu, or Ctrl+Q.",
            )
            .small()
            .color(theme::DIM),
        );
        ui.separator();
        ui.heading("General");
        ui.checkbox(&mut self.cfg.fullscreen, "Start OrbitOS fullscreen");
        ui.horizontal(|ui| {
            ui.label("Weather city");
            ui.text_edit_singleline(&mut self.cfg.weather_city);
            if ui.small_button("Update").clicked() {
                self.center.weather_at = None;
            }
        });
        if ui.button("Show the welcome tour again").clicked() {
            self.welcome.open = true;
            self.welcome.step = 0;
        }
        ui.separator();
        ui.label(
            RichText::new(format!(
                "OrbitOS {} · {} · written in Rust",
                env!("CARGO_PKG_VERSION"),
                orbit_core::host_os()
            ))
            .strong(),
        );
        ui.label(RichText::new("Shortcuts: Ctrl/Cmd+Space command bar · Ctrl/Cmd+K Claude · F9 push-to-talk · Ctrl+Alt+arrows snap windows · Ctrl+Alt+1-4 desktops").small().color(theme::DIM));
    }

    fn confirm_restore_dialog(&mut self, ctx: &egui::Context) {
        let Some(p) = self.settings.confirm_restore.clone() else {
            return;
        };
        let mut decision = None;
        egui::Window::new("Restore this backup?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(p.file_name().unwrap_or_default().to_string_lossy());
                ui.label(RichText::new("Your current settings, chats and playlists will be replaced. OrbitOS will reload its settings.").color(theme::WARN));
                ui.horizontal(|ui| {
                    if ui.button("Restore").clicked() {
                        decision = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        decision = Some(false);
                    }
                });
            });
        match decision {
            Some(true) => {
                self.settings.confirm_restore = None;
                match backup::restore(&p, &self.settings.restore_password) {
                    Ok(n) => {
                        self.cfg = Config::load();
                        self.key_edit = self.cfg.api_key.clone().unwrap_or_default();
                        theme::apply(ctx, &self.cfg);
                        self.chat = Self::load_current_chat();
                        self.claude.cards.clear();
                        self.wm = super::wm::Wm::load();
                        self.settings.backup_status = Some(format!("Restored {n} files"));
                        self.push_note("Backup restored", &format!("{n} files"));
                    }
                    Err(e) => self.settings.backup_status = Some(format!("Restore failed: {e}")),
                }
            }
            Some(false) => self.settings.confirm_restore = None,
            None => {}
        }
    }
}
