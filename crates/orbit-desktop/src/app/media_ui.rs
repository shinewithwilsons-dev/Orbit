//! Media window: music library with an in-app player, and videos played in
//! mpv (or VLC).

use super::*;
use orbit_core::media::{self, Item, Kind};

pub(super) struct MediaState {
    pub tab: Kind,
    pub music: Vec<Item>,
    pub videos: Vec<Item>,
    pub loaded: bool,
    pub loading: bool,
    pub filter: String,
    pub open_path: String,
    pub players: Vec<&'static str>,
    #[cfg(feature = "audio")]
    pub audio: Option<media::AudioPlayer>,
    pub audio_err: Option<String>,
}

impl MediaState {
    pub fn new() -> Self {
        Self {
            tab: Kind::Audio,
            music: Vec::new(),
            videos: Vec::new(),
            loaded: false,
            loading: false,
            filter: String::new(),
            open_path: String::new(),
            players: media::players().iter().map(|p| p.name).collect(),
            #[cfg(feature = "audio")]
            audio: None,
            audio_err: None,
        }
    }
}

impl Desktop {
    pub(super) fn open_media(&mut self) {
        self.win.media = true;
        if !self.media.loaded && !self.media.loading {
            self.rescan_media();
        }
    }

    fn rescan_media(&mut self) {
        self.media.loading = true;
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let music = media::scan(&media::library_dirs(Kind::Audio), Kind::Audio);
            let videos = media::scan(&media::library_dirs(Kind::Video), Kind::Video);
            let _ = tx.send(Event::MediaLibrary(music, videos));
            ctx.request_repaint();
        });
    }

    pub(super) fn on_media_library(&mut self, music: Vec<Item>, videos: Vec<Item>) {
        self.media.music = music;
        self.media.videos = videos;
        self.media.loaded = true;
        self.media.loading = false;
    }

    /// Play any media file: music in-app, video in mpv/VLC.
    pub(super) fn play_path(&mut self, path: PathBuf) {
        self.win.media = true;
        match media::kind_of(&path) {
            Some(Kind::Audio) => self.play_audio(vec![path], 0),
            _ => self.play_video(&path),
        }
    }

    fn play_video(&mut self, path: &std::path::Path) {
        match media::play_external(&[path.to_path_buf()]) {
            Ok(by) => self.notify(format!("Playing in {by}")),
            Err(e) => self.notify(format!("Could not play: {e}")),
        }
    }

    #[cfg(feature = "audio")]
    fn ensure_audio(&mut self) -> bool {
        if self.media.audio.is_none() && self.media.audio_err.is_none() {
            match media::AudioPlayer::new(self.cfg.volume) {
                Ok(p) => self.media.audio = Some(p),
                Err(e) => self.media.audio_err = Some(e.to_string()),
            }
        }
        self.media.audio.is_some()
    }

    fn play_audio(&mut self, queue: Vec<PathBuf>, start: usize) {
        #[cfg(feature = "audio")]
        if self.ensure_audio() {
            let res = self
                .media
                .audio
                .as_mut()
                .map(|a| a.play_queue(queue.clone(), start));
            match res {
                Some(Ok(())) => return,
                Some(Err(e)) => self.notify(format!("In-app player: {e}. Trying mpv/VLC…")),
                None => {}
            }
        }
        // Fallback: hand the playlist to mpv / VLC.
        let rest: Vec<PathBuf> = queue.into_iter().skip(start).collect();
        match media::play_external(&rest) {
            Ok(by) => self.notify(format!("Playing in {by}")),
            Err(e) => self.notify(format!("Could not play: {e}")),
        }
    }

    pub(super) fn media_tick(&mut self) {
        #[cfg(feature = "audio")]
        if let Some(a) = self.media.audio.as_mut() {
            a.tick();
        }
    }

    fn now_playing(&mut self, ui: &mut egui::Ui) {
        #[cfg(feature = "audio")]
        {
            let mut vol = self.cfg.volume;
            let mut vol_changed = false;
            if let Some(a) = self.media.audio.as_mut() {
                if let Some(cur) = a.current().cloned() {
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let title = cur
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_default();
                        ui.label(
                            RichText::new(title)
                                .size(17.0)
                                .strong()
                                .color(theme::MEDIA_PINK),
                        );
                        let pos = a.position();
                        match a.total {
                            Some(total) if total.as_secs() > 0 => {
                                let mut t = pos.as_secs_f32();
                                let resp = ui.add(
                                    egui::Slider::new(&mut t, 0.0..=total.as_secs_f32())
                                        .show_value(false)
                                        .text(format!(
                                            "{} / {}",
                                            media::fmt_duration(pos),
                                            media::fmt_duration(total)
                                        )),
                                );
                                if resp.drag_stopped() || resp.clicked() {
                                    a.seek(Duration::from_secs_f32(t));
                                }
                            }
                            _ => {
                                ui.label(RichText::new(media::fmt_duration(pos)).color(theme::DIM));
                            }
                        }
                        ui.horizontal(|ui| {
                            if ui.button("⏮ Prev").clicked() {
                                let _ = a.prev();
                            }
                            if ui
                                .button(if a.is_paused() {
                                    "▶ Play"
                                } else {
                                    "⏸ Pause"
                                })
                                .clicked()
                            {
                                a.toggle();
                            }
                            if ui.button("⏭ Next").clicked() {
                                let _ = a.next();
                            }
                            if ui.button("⏹ Stop").clicked() {
                                a.stop();
                            }
                            ui.separator();
                            if ui
                                .add(
                                    egui::Slider::new(&mut vol, 0.0..=1.0)
                                        .show_value(false)
                                        .text("Volume"),
                                )
                                .changed()
                            {
                                a.set_volume(vol);
                                vol_changed = true;
                            }
                            if let Some(i) = a.index {
                                ui.label(
                                    RichText::new(format!("{} of {}", i + 1, a.queue.len()))
                                        .color(theme::DIM),
                                );
                            }
                        });
                    });
                }
            }
            if vol_changed {
                self.cfg.volume = vol;
            }
        }
        let _ = ui;
    }

    pub(super) fn media_window(&mut self, ctx: &egui::Context) {
        let mut open = self.win.media;
        let mut play: Option<(Kind, Vec<PathBuf>, usize)> = None;
        let mut rescan = false;
        egui::Window::new(RichText::new("Media").color(theme::MEDIA_PINK))
            .id(egui::Id::new("media_win"))
            .open(&mut open)
            .default_size([500.0, 330.0])
            .default_pos([224.0, 410.0])
            .show(ctx, |ui| {
                self.now_playing(ui);
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut self.media.tab,
                        Kind::Audio,
                        format!("Music ({})", self.media.music.len()),
                    );
                    ui.selectable_value(
                        &mut self.media.tab,
                        Kind::Video,
                        format!("Videos ({})", self.media.videos.len()),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.media.filter)
                            .hint_text("Search…")
                            .desired_width(160.0),
                    );
                    if ui.button("Rescan").clicked() {
                        rescan = true;
                    }
                    if self.media.loading {
                        ui.spinner();
                    }
                });
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.media.open_path)
                            .hint_text("Play any file or URL…")
                            .desired_width(ui.available_width() - 60.0),
                    );
                    if ui.button("Play").clicked() && !self.media.open_path.trim().is_empty() {
                        let p = orbit_core::expand_tilde(self.media.open_path.trim());
                        let k = media::kind_of(&p).unwrap_or(Kind::Video);
                        play = Some((k, vec![p], 0));
                    }
                });
                // Player status.
                let video_player = self.media.players.first().copied();
                let status = match video_player {
                    Some(p) => format!("Videos play in {p}"),
                    None => format!(
                        "No video player found. Install mpv: {}",
                        media::player_install_hint()
                    ),
                };
                let music_status = if cfg!(feature = "audio") {
                    match &self.media.audio_err {
                        Some(e) => format!("Music: {e}, so music opens in mpv/VLC"),
                        None => "Music plays in OrbitOS".to_string(),
                    }
                } else {
                    "Music opens in mpv/VLC".to_string()
                };
                ui.label(
                    RichText::new(format!("{status} · {music_status}"))
                        .small()
                        .color(theme::DIM),
                );
                ui.separator();

                let q = self.media.filter.to_lowercase();
                let items: Vec<&Item> = match self.media.tab {
                    Kind::Audio => &self.media.music,
                    Kind::Video => &self.media.videos,
                }
                .iter()
                .filter(|i| {
                    q.is_empty()
                        || i.title.to_lowercase().contains(&q)
                        || i.folder.to_lowercase().contains(&q)
                })
                .collect();
                let paths: Vec<PathBuf> = items.iter().map(|i| i.path.clone()).collect();
                #[cfg(feature = "audio")]
                let current = self.media.audio.as_ref().and_then(|a| a.current().cloned());
                #[cfg(not(feature = "audio"))]
                let current: Option<PathBuf> = None;

                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        if items.is_empty() && self.media.loaded {
                            let folders = media::library_dirs(self.media.tab)
                                .iter()
                                .map(|d| d.display().to_string())
                                .collect::<Vec<_>>()
                                .join(", ");
                            ui.label(
                                RichText::new(format!(
                                    "Nothing found in {}. Add files there and press Rescan.",
                                    if folders.is_empty() {
                                        "your Music/Videos folders".into()
                                    } else {
                                        folders
                                    }
                                ))
                                .color(theme::DIM),
                            );
                        }
                        egui::Grid::new("media_list")
                            .num_columns(3)
                            .striped(true)
                            .show(ui, |ui| {
                                for (idx, it) in items.iter().enumerate() {
                                    let playing = current.as_ref() == Some(&it.path);
                                    let label = if playing {
                                        RichText::new(format!("▶ {}", it.title))
                                            .strong()
                                            .color(theme::MEDIA_PINK)
                                    } else {
                                        RichText::new(&it.title)
                                    };
                                    let r = ui
                                        .add(egui::Label::new(label).sense(egui::Sense::click()))
                                        .on_hover_text("Double-click to play");
                                    if r.double_clicked() {
                                        play = Some((self.media.tab, paths.clone(), idx));
                                    }
                                    ui.label(RichText::new(&it.folder).color(theme::DIM));
                                    ui.label(RichText::new(human_bytes(it.size)).color(theme::DIM));
                                    ui.end_row();
                                }
                            });
                    });
            });
        self.win.media = open;
        if rescan {
            self.media.players = media::players().iter().map(|p| p.name).collect();
            self.rescan_media();
        }
        if let Some((kind, list, idx)) = play {
            match kind {
                Kind::Audio => self.play_audio(list, idx),
                Kind::Video => {
                    if let Some(p) = list.get(idx).cloned() {
                        self.play_video(&p);
                    }
                }
            }
        }
    }
}
