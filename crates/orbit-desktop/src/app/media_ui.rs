//! Media window: music (album art, artist/album, equaliser, playlists),
//! videos in mpv/VLC, internet radio, and your BeMusic site.

use super::icons::WinId;
use super::*;
use orbit_core::media::{self, Item, Kind};
use orbit_core::radio::{self, RemoteTrack, Station, StreamPlayer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum View {
    Music,
    Videos,
    Playlists,
    Radio,
    BeMusic,
}

pub(super) struct MediaState {
    pub view: View,
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
    pub stream: StreamPlayer,
    pub art: Option<(PathBuf, Option<egui::TextureHandle>)>,
    pub playlist: Option<String>,
    pub new_playlist: String,
    pub radio_query: String,
    pub radio_results: Vec<Station>,
    pub radio_loading: bool,
    pub be_query: String,
    pub be_results: Vec<RemoteTrack>,
    pub be_loading: bool,
    pub be_err: Option<String>,
    pub show_eq: bool,
}

impl MediaState {
    pub fn new() -> Self {
        Self {
            view: View::Music,
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
            stream: StreamPlayer::default(),
            art: None,
            playlist: None,
            new_playlist: String::new(),
            radio_query: String::new(),
            radio_results: Vec::new(),
            radio_loading: false,
            be_query: String::new(),
            be_results: Vec::new(),
            be_loading: false,
            be_err: None,
            show_eq: false,
        }
    }
}

const EQ_PRESETS: [(&str, [f32; 3]); 5] = [
    ("Flat", [0.0, 0.0, 0.0]),
    ("Bass boost", [6.0, 0.0, -1.0]),
    ("Vocal", [-2.0, 4.0, 1.0]),
    ("Treble", [-1.0, 0.0, 5.0]),
    ("Loudness", [5.0, -1.0, 4.0]),
];

impl Desktop {
    pub(super) fn open_media(&mut self) {
        self.win.media = true;
        if !self.media.loaded && !self.media.loading {
            self.rescan_media();
        }
    }

    pub(super) fn rescan_media(&mut self) {
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

    /// Radio or BeMusic stream.
    pub(super) fn play_stream(&mut self, url: &str, title: &str, headers: Option<String>) {
        #[cfg(feature = "audio")]
        if let Some(a) = self.media.audio.as_mut() {
            a.stop();
        }
        match self.media.stream.play(url, title, headers.as_deref()) {
            Ok(()) => self.push_note("Now playing", title),
            Err(e) => self.notify(e.to_string()),
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
                Ok(p) => {
                    p.eq.set(self.cfg.eq);
                    self.media.audio = Some(p)
                }
                Err(e) => self.media.audio_err = Some(e.to_string()),
            }
        }
        self.media.audio.is_some()
    }

    pub(super) fn play_audio(&mut self, queue: Vec<PathBuf>, start: usize) {
        self.media.stream.stop();
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
            a.eq.set(self.cfg.eq);
            // Load cover art when the track changes.
            let cur = a.current().cloned();
            if cur.as_ref() != self.media.art.as_ref().map(|(p, _)| p) {
                self.media.art = cur.map(|p| {
                    let tex = media::read_tags(&p, true)
                        .and_then(|t| t.art)
                        .and_then(|bytes| {
                            let img = image::load_from_memory(&bytes)
                                .ok()?
                                .thumbnail(256, 256)
                                .to_rgba8();
                            let ci = egui::ColorImage::from_rgba_unmultiplied(
                                [img.width() as usize, img.height() as usize],
                                img.as_raw(),
                            );
                            Some(
                                self.ctx
                                    .load_texture("cover", ci, egui::TextureOptions::LINEAR),
                            )
                        });
                    (p, tex)
                });
            }
        }
    }

    fn now_playing(&mut self, ui: &mut egui::Ui) {
        if self.media.stream.is_playing() {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(48.0, 48.0), egui::Sense::hover());
                    theme::paint_icon(ui.painter(), rect, theme::Glyph::Media, "");
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Streaming").small().color(theme::DIM));
                        ui.label(
                            RichText::new(&self.media.stream.title)
                                .strong()
                                .color(theme::MEDIA_PINK),
                        );
                        if ui.button("⏹ Stop").clicked() {
                            self.media.stream.stop();
                        }
                    });
                });
            });
            return;
        }
        #[cfg(feature = "audio")]
        {
            let mut vol = self.cfg.volume;
            let mut vol_changed = false;
            let art = self.media.art.as_ref().and_then(|(_, t)| t.clone());
            let tags = self
                .media
                .audio
                .as_ref()
                .and_then(|a| a.current().cloned())
                .and_then(|p| {
                    self.media
                        .music
                        .iter()
                        .find(|m| m.path == p)
                        .map(|m| (m.title.clone(), m.artist.clone(), m.album.clone()))
                });
            if let Some(a) = self.media.audio.as_mut() {
                if let Some(cur) = a.current().cloned() {
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let (rect, _) = ui
                                .allocate_exact_size(egui::vec2(72.0, 72.0), egui::Sense::hover());
                            match &art {
                                Some(t) => {
                                    ui.painter().image(
                                        t.id(),
                                        rect,
                                        egui::Rect::from_min_max(
                                            egui::pos2(0.0, 0.0),
                                            egui::pos2(1.0, 1.0),
                                        ),
                                        Color32::WHITE,
                                    );
                                }
                                None => {
                                    theme::paint_icon(ui.painter(), rect, theme::Glyph::Media, "")
                                }
                            }
                            ui.vertical(|ui| {
                                let (title, artist, album) = tags.unwrap_or_else(|| {
                                    (
                                        cur.file_stem()
                                            .map(|s| s.to_string_lossy().to_string())
                                            .unwrap_or_default(),
                                        String::new(),
                                        String::new(),
                                    )
                                });
                                ui.label(
                                    RichText::new(title)
                                        .size(17.0)
                                        .strong()
                                        .color(theme::MEDIA_PINK),
                                );
                                let sub = [artist, album]
                                    .into_iter()
                                    .filter(|s| !s.is_empty())
                                    .collect::<Vec<_>>()
                                    .join(" · ");
                                if !sub.is_empty() {
                                    ui.label(RichText::new(sub).color(theme::DIM));
                                }
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
                                        ui.label(
                                            RichText::new(media::fmt_duration(pos))
                                                .color(theme::DIM),
                                        );
                                    }
                                }
                            });
                        });
                        ui.horizontal(|ui| {
                            if ui.button("⏮").on_hover_text("Previous").clicked() {
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
                            if ui.button("⏭").on_hover_text("Next").clicked() {
                                let _ = a.next();
                            }
                            if ui.button("⏹").on_hover_text("Stop").clicked() {
                                a.stop();
                            }
                            ui.separator();
                            if ui
                                .add(
                                    egui::Slider::new(&mut vol, 0.0..=1.0)
                                        .show_value(false)
                                        .text("Vol"),
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

    fn eq_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for (i, n) in ["Bass", "Mid", "Treble"].iter().enumerate() {
                ui.vertical(|ui| {
                    ui.add(
                        egui::Slider::new(&mut self.cfg.eq[i], -12.0..=12.0)
                            .vertical()
                            .show_value(false),
                    );
                    ui.label(RichText::new(format!("{n}\n{:+.0} dB", self.cfg.eq[i])).small());
                });
            }
            ui.vertical(|ui| {
                ui.label(RichText::new("Presets").small().strong());
                for (n, g) in EQ_PRESETS {
                    if ui.small_button(n).clicked() {
                        self.cfg.eq = g;
                    }
                }
            });
        });
        if !cfg!(feature = "audio") {
            ui.label(
                RichText::new("The equaliser works with the in-app player.")
                    .small()
                    .color(theme::DIM),
            );
        }
    }

    pub(super) fn media_window(&mut self, ctx: &egui::Context) {
        let mut open = self.win.media;
        if !self.wm_visible(WinId::Media) {
            return;
        }
        let mut play: Option<(Kind, Vec<PathBuf>, usize)> = None;
        let mut stream: Option<(String, String, Option<String>)> = None;
        let mut rescan = false;
        let mut add_to: Option<(String, PathBuf)> = None;
        let mut radio_search = false;
        let mut be_search = false;

        self.wm_window(WinId::Media, RichText::new("Media").color(theme::MEDIA_PINK), [224.0, 300.0, 620.0, 470.0])
            .open(&mut open)
            .show(ctx, |ui| {
                self.now_playing(ui);
                ui.horizontal(|ui| {
                    for (v, n) in [
                        (View::Music, format!("Music ({})", self.media.music.len())),
                        (View::Videos, format!("Videos ({})", self.media.videos.len())),
                        (View::Playlists, "Playlists".into()),
                        (View::Radio, "Radio".into()),
                        (View::BeMusic, "BeMusic".into()),
                    ] {
                        ui.selectable_value(&mut self.media.view, v, n);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.toggle_value(&mut self.media.show_eq, "EQ");
                    });
                });
                if self.media.show_eq {
                    egui::Frame::group(ui.style()).show(ui, |ui| self.eq_panel(ui));
                }
                ui.separator();
                match self.media.view {
                    View::Music | View::Videos => {
                        let kind = if self.media.view == View::Music { Kind::Audio } else { Kind::Video };
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(&mut self.media.filter).hint_text("Search title, artist, album…").desired_width(220.0));
                            if ui.button("Rescan").clicked() {
                                rescan = true;
                            }
                            if self.media.loading {
                                ui.spinner();
                            }
                            ui.add(egui::TextEdit::singleline(&mut self.media.open_path).hint_text("Play a file or URL…").desired_width(ui.available_width() - 60.0));
                            if ui.button("Play").clicked() && !self.media.open_path.trim().is_empty() {
                                let s = self.media.open_path.trim().to_string();
                                if s.starts_with("http") {
                                    stream = Some((s.clone(), s, None));
                                } else {
                                    let p = orbit_core::expand_tilde(&s);
                                    play = Some((media::kind_of(&p).unwrap_or(Kind::Video), vec![p], 0));
                                }
                            }
                        });
                        let status = match self.media.players.first() {
                            Some(p) => format!("Videos play in {p}"),
                            None => format!("No video player found: {}", media::player_install_hint()),
                        };
                        let music_status = if cfg!(feature = "audio") {
                            match &self.media.audio_err {
                                Some(e) => format!("music: {e}, using mpv/VLC"),
                                None => "music plays in OrbitOS".into(),
                            }
                        } else {
                            "music opens in mpv/VLC".into()
                        };
                        ui.label(RichText::new(format!("{status} · {music_status}")).small().color(theme::DIM));
                        let q = self.media.filter.to_lowercase();
                        let list = if kind == Kind::Audio { &self.media.music } else { &self.media.videos };
                        let items: Vec<Item> = list
                            .iter()
                            .filter(|i| q.is_empty() || [&i.title, &i.artist, &i.album, &i.folder].iter().any(|s| s.to_lowercase().contains(&q)))
                            .cloned()
                            .collect();
                        let paths: Vec<PathBuf> = items.iter().map(|i| i.path.clone()).collect();
                        #[cfg(feature = "audio")]
                        let current = self.media.audio.as_ref().and_then(|a| a.current().cloned());
                        #[cfg(not(feature = "audio"))]
                        let current: Option<PathBuf> = None;
                        let playlists = media::list_playlists();
                        egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
                            if items.is_empty() && self.media.loaded {
                                ui.label(RichText::new("Nothing found. Add files to your Music or Videos folder and press Rescan.").color(theme::DIM));
                            }
                            egui::Grid::new("media_list").num_columns(3).striped(true).show(ui, |ui| {
                                for (idx, it) in items.iter().enumerate() {
                                    let playing = current.as_ref() == Some(&it.path);
                                    let label = if playing { RichText::new(format!("▶ {}", it.title)).strong().color(theme::MEDIA_PINK) } else { RichText::new(&it.title) };
                                    let r = ui.add(egui::Label::new(label).sense(egui::Sense::click())).on_hover_text("Double-click to play · right-click for more");
                                    if r.double_clicked() {
                                        play = Some((kind, paths.clone(), idx));
                                    }
                                    r.context_menu(|ui| {
                                        if ui.button("Play").clicked() {
                                            play = Some((kind, paths.clone(), idx));
                                            ui.close_menu();
                                        }
                                        if kind == Kind::Audio {
                                            ui.menu_button("Add to playlist", |ui| {
                                                for p in &playlists {
                                                    if ui.button(p).clicked() {
                                                        add_to = Some((p.clone(), it.path.clone()));
                                                        ui.close_menu();
                                                    }
                                                }
                                                ui.horizontal(|ui| {
                                                    ui.add(egui::TextEdit::singleline(&mut self.media.new_playlist).hint_text("New playlist").desired_width(120.0));
                                                    if ui.button("Add").clicked() && !self.media.new_playlist.trim().is_empty() {
                                                        add_to = Some((self.media.new_playlist.trim().to_string(), it.path.clone()));
                                                        self.media.new_playlist.clear();
                                                        ui.close_menu();
                                                    }
                                                });
                                            });
                                        }
                                    });
                                    let sub = if kind == Kind::Audio && !it.artist.is_empty() {
                                        if it.album.is_empty() { it.artist.clone() } else { format!("{} · {}", it.artist, it.album) }
                                    } else {
                                        it.folder.clone()
                                    };
                                    ui.label(RichText::new(sub).color(theme::DIM));
                                    ui.label(RichText::new(human_bytes(it.size)).small().color(theme::DIM));
                                    ui.end_row();
                                }
                            });
                        });
                    }
                    View::Playlists => {
                        let lists = media::list_playlists();
                        ui.horizontal(|ui| {
                            ui.add(egui::TextEdit::singleline(&mut self.media.new_playlist).hint_text("New playlist name").desired_width(200.0));
                            if ui.button("Create").clicked() && !self.media.new_playlist.trim().is_empty() {
                                let _ = media::save_playlist(self.media.new_playlist.trim(), &[]);
                                self.media.playlist = Some(orbit_core::office::safe_name(self.media.new_playlist.trim()));
                                self.media.new_playlist.clear();
                            }
                        });
                        ui.label(RichText::new("Add songs from the Music tab: right-click a track -> Add to playlist.").small().color(theme::DIM));
                        ui.columns(2, |cols| {
                            egui::ScrollArea::vertical().id_source("pl_names").show(&mut cols[0], |ui| {
                                if lists.is_empty() {
                                    ui.label(RichText::new("No playlists yet.").color(theme::DIM));
                                }
                                for l in &lists {
                                    if ui.selectable_label(self.media.playlist.as_ref() == Some(l), l).clicked() {
                                        self.media.playlist = Some(l.clone());
                                    }
                                }
                            });
                            if let Some(name) = self.media.playlist.clone() {
                                let tracks = media::load_playlist(&name);
                                let ui = &mut cols[1];
                                ui.horizontal(|ui| {
                                    if ui.add_enabled(!tracks.is_empty(), egui::Button::new("▶ Play all")).clicked() {
                                        play = Some((Kind::Audio, tracks.clone(), 0));
                                    }
                                    if ui.button("Delete playlist").clicked() {
                                        let _ = media::delete_playlist(&name);
                                        self.media.playlist = None;
                                    }
                                });
                                egui::ScrollArea::vertical().id_source("pl_tracks").show(ui, |ui| {
                                    let mut remove = None;
                                    for (i, t) in tracks.iter().enumerate() {
                                        ui.horizontal(|ui| {
                                            let n = t.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                                            if ui.add(egui::Label::new(n).sense(egui::Sense::click())).double_clicked() {
                                                play = Some((Kind::Audio, tracks.clone(), i));
                                            }
                                            if ui.small_button("✕").clicked() {
                                                remove = Some(i);
                                            }
                                        });
                                    }
                                    if let Some(i) = remove {
                                        let mut v = tracks.clone();
                                        v.remove(i);
                                        let _ = media::save_playlist(&name, &v);
                                    }
                                });
                            }
                        });
                    }
                    View::Radio => {
                        ui.horizontal(|ui| {
                            let r = ui.add(egui::TextEdit::singleline(&mut self.media.radio_query).hint_text("Search 50,000+ stations: jazz, BBC, ambient…").desired_width(300.0));
                            if (ui.button("Search").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))) && !self.media.radio_query.trim().is_empty() {
                                radio_search = true;
                            }
                            if self.media.radio_loading {
                                ui.spinner();
                            }
                        });
                        egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
                            ui.label(RichText::new("Featured").strong());
                            for s in radio::featured() {
                                station_row(ui, &s, &mut stream);
                            }
                            if !self.media.radio_results.is_empty() {
                                ui.add_space(6.0);
                                ui.label(RichText::new("Results").strong());
                                for s in &self.media.radio_results {
                                    station_row(ui, s, &mut stream);
                                }
                            }
                            ui.label(RichText::new("Station directory by radio-browser.info. Streams play through mpv or VLC.").small().color(theme::DIM));
                        });
                    }
                    View::BeMusic => {
                        if self.cfg.bemusic_url.is_none() {
                            ui.label("Connect your BeMusic site in Settings -> Media (site URL and API token).");
                            if ui.button("Open Settings").clicked() {
                                self.settings_tab = super::settings_ui::Tab::Media;
                                self.win.settings = true;
                            }
                        } else {
                            ui.horizontal(|ui| {
                                let r = ui.add(egui::TextEdit::singleline(&mut self.media.be_query).hint_text("Search your BeMusic library…").desired_width(300.0));
                                if (ui.button("Search").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))) && !self.media.be_query.trim().is_empty() {
                                    be_search = true;
                                }
                                if self.media.be_loading {
                                    ui.spinner();
                                }
                            });
                            if let Some(e) = &self.media.be_err {
                                ui.colored_label(theme::WARN, e);
                            }
                            let header = self.cfg.bemusic_token.as_ref().map(|t| format!("Authorization: Bearer {t}"));
                            egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
                                for t in &self.media.be_results {
                                    ui.horizontal(|ui| {
                                        if ui.small_button("▶").clicked() {
                                            stream = Some((t.url.clone(), format!("{} · {}", t.title, t.artist), header.clone()));
                                        }
                                        ui.label(RichText::new(&t.title).strong());
                                        ui.label(RichText::new(&t.artist).color(theme::DIM));
                                    });
                                }
                            });
                        }
                    }
                }
            });
        self.win.media = open;

        if rescan {
            self.media.players = media::players().iter().map(|p| p.name).collect();
            self.rescan_media();
        }
        if let Some((name, p)) = add_to {
            match media::add_to_playlist(&name, &p) {
                Ok(()) => self.notify(format!("Added to {name}")),
                Err(e) => self.notify(e.to_string()),
            }
        }
        if let Some((url, title, h)) = stream {
            self.play_stream(&url, &title, h);
        }
        if radio_search {
            self.media.radio_loading = true;
            let q = self.media.radio_query.clone();
            let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
            std::thread::spawn(move || {
                let _ = tx.send(Event::RadioResults(
                    radio::search(&q).map_err(|e| e.to_string()),
                ));
                ctx.request_repaint();
            });
        }
        if be_search {
            self.media.be_loading = true;
            let (q, cfg) = (self.media.be_query.clone(), self.cfg.clone());
            let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
            std::thread::spawn(move || {
                let _ = tx.send(Event::BeMusicResults(
                    radio::bemusic_search(&cfg, &q).map_err(|e| e.to_string()),
                ));
                ctx.request_repaint();
            });
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

fn station_row(ui: &mut egui::Ui, s: &Station, out: &mut Option<(String, String, Option<String>)>) {
    ui.horizontal(|ui| {
        if ui.small_button("▶").clicked() {
            *out = Some((s.url.clone(), s.name.clone(), None));
        }
        ui.label(RichText::new(&s.name).strong());
        ui.label(RichText::new(&s.info).small().color(theme::DIM));
    });
}
