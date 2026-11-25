//! Window manager: snap to halves/quarters, maximise, minimise to the dock,
//! four virtual desktops, and window positions remembered between runs.
//!
//! Shortcuts (Ctrl+Alt on Windows/Linux, Cmd+Alt on macOS):
//!   ←/-> snap left/right · ↑ maximise · ↓ restore · M minimise
//!   1-4 switch desktop · Shift+1-4 move the focused window there

use super::icons::WinId;
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct WinState {
    pub gen: u32,
    pub desktop: u8,
    pub minimized: bool,
    pub rect: Option<[f32; 4]>,
    pub before_snap: Option<[f32; 4]>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct Wm {
    pub desktop: u8,
    pub states: HashMap<String, WinState>,
    #[serde(skip)]
    pub focused: Option<WinId>,
    #[serde(skip)]
    pub last_save: Option<std::time::SystemTime>,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum Zone {
    Left,
    Right,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Max,
}

pub(super) const MANAGED: [WinId; 9] = [
    WinId::Files,
    WinId::Terminal,
    WinId::Claude,
    WinId::Shield,
    WinId::Media,
    WinId::Office,
    WinId::Store,
    WinId::Monitor,
    WinId::Settings,
];

fn wm_path() -> PathBuf {
    Config::data_dir().join("windows.json")
}

impl Wm {
    pub fn load() -> Self {
        std::fs::read_to_string(wm_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(wm_path(), s);
        }
    }

    pub fn state(&mut self, key: WinId) -> &mut WinState {
        let d = self.desktop;
        self.states
            .entry(key.name().to_string())
            .or_insert_with(|| WinState {
                desktop: d,
                ..Default::default()
            })
    }

    pub fn peek(&self, key: WinId) -> Option<&WinState> {
        self.states.get(key.name())
    }
}

fn to_rect(r: [f32; 4]) -> egui::Rect {
    egui::Rect::from_min_size(egui::pos2(r[0], r[1]), egui::vec2(r[2], r[3]))
}

fn from_rect(r: egui::Rect) -> [f32; 4] {
    [r.min.x, r.min.y, r.width(), r.height()]
}

impl Desktop {
    pub(super) fn win_flag(&mut self, key: WinId) -> &mut bool {
        match key {
            WinId::Files => &mut self.win.files,
            WinId::Terminal => &mut self.win.terminal,
            WinId::Claude => &mut self.win.claude,
            WinId::Shield => &mut self.win.shield,
            WinId::Media => &mut self.win.media,
            WinId::Office => &mut self.win.office,
            WinId::Store => &mut self.win.store,
            WinId::Monitor => &mut self.win.monitor,
            WinId::Settings => &mut self.win.settings,
        }
    }

    fn is_open(&mut self, key: WinId) -> bool {
        *self.win_flag(key)
    }

    /// Window id; changing `gen` makes egui place it fresh at a new rect.
    fn win_id(&self, key: WinId) -> egui::Id {
        let gen = self.wm.peek(key).map(|s| s.gen).unwrap_or(0);
        egui::Id::new(("orbit_win", key.name(), gen))
    }

    /// A window builder with the remembered (or snapped) position.
    pub(super) fn wm_window(
        &mut self,
        key: WinId,
        title: impl Into<egui::WidgetText>,
        default: [f32; 4],
    ) -> egui::Window<'static> {
        let id = self.win_id(key);
        let mut rect = self.wm.state(key).rect.unwrap_or(default);
        // Keep windows on screen, whatever its size.
        let d = self.desk_rect;
        if d.width() > 200.0 && d.height() > 200.0 {
            rect[2] = rect[2].min(d.width() - 16.0);
            rect[3] = rect[3].min(d.height() - 16.0);
            rect[0] = rect[0].clamp(d.min.x + 4.0, (d.max.x - rect[2] - 4.0).max(d.min.x + 4.0));
            rect[1] = rect[1].clamp(d.min.y + 4.0, (d.max.y - rect[3] - 4.0).max(d.min.y + 4.0));
        }
        egui::Window::new(title).id(id).default_rect(to_rect(rect))
    }

    /// Shown on this desktop and not minimised.
    pub(super) fn wm_visible(&mut self, key: WinId) -> bool {
        let desk = self.wm.desktop;
        let st = self.wm.state(key);
        !st.minimized && st.desktop == desk
    }

    /// Bring a window up on the current desktop.
    pub(super) fn wm_show(&mut self, key: WinId) {
        let desk = self.wm.desktop;
        let st = self.wm.state(key);
        st.minimized = false;
        st.desktop = desk;
        self.wm.focused = Some(key);
    }

    pub(super) fn wm_minimize(&mut self, key: WinId) {
        self.wm.state(key).minimized = true;
        if self.wm.focused == Some(key) {
            self.wm.focused = None;
        }
    }

    pub(super) fn snap(&mut self, key: WinId, zone: Zone) {
        let a = self.desk_rect.shrink(6.0);
        let (w2, h2) = (a.width() / 2.0, a.height() / 2.0);
        let r = match zone {
            Zone::Left => egui::Rect::from_min_size(a.min, egui::vec2(w2 - 3.0, a.height())),
            Zone::Right => egui::Rect::from_min_size(
                a.min + egui::vec2(w2 + 3.0, 0.0),
                egui::vec2(w2 - 3.0, a.height()),
            ),
            Zone::TopLeft => egui::Rect::from_min_size(a.min, egui::vec2(w2 - 3.0, h2 - 3.0)),
            Zone::TopRight => egui::Rect::from_min_size(
                a.min + egui::vec2(w2 + 3.0, 0.0),
                egui::vec2(w2 - 3.0, h2 - 3.0),
            ),
            Zone::BottomLeft => egui::Rect::from_min_size(
                a.min + egui::vec2(0.0, h2 + 3.0),
                egui::vec2(w2 - 3.0, h2 - 3.0),
            ),
            Zone::BottomRight => egui::Rect::from_min_size(
                a.min + egui::vec2(w2 + 3.0, h2 + 3.0),
                egui::vec2(w2 - 3.0, h2 - 3.0),
            ),
            Zone::Max => a,
        };
        // Leave room for the title bar egui adds above the content.
        let r = egui::Rect::from_min_max(r.min, r.max - egui::vec2(0.0, 30.0));
        let st = self.wm.state(key);
        if st.before_snap.is_none() {
            st.before_snap = st.rect;
        }
        st.rect = Some(from_rect(r));
        st.gen += 1;
        st.minimized = false;
    }

    pub(super) fn restore(&mut self, key: WinId) {
        let st = self.wm.state(key);
        if let Some(r) = st.before_snap.take() {
            st.rect = Some(r);
            st.gen += 1;
        }
        st.minimized = false;
    }

    pub(super) fn move_to_desktop(&mut self, key: WinId, d: u8) {
        let st = self.wm.state(key);
        st.desktop = d;
        st.minimized = false;
    }

    /// After drawing: remember where windows are and which one has focus.
    pub(super) fn wm_track(&mut self, ctx: &egui::Context) {
        for key in MANAGED {
            if !self.is_open(key) || !self.wm_visible(key) {
                continue;
            }
            let id = self.win_id(key);
            if let Some(r) = ctx.memory(|m| m.area_rect(id)) {
                self.wm.state(key).rect = Some(from_rect(r));
            }
        }
        if ctx.input(|i| i.pointer.any_pressed()) {
            if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
                if let Some(layer) = ctx.layer_id_at(pos) {
                    self.wm.focused = MANAGED.into_iter().find(|k| layer.id == self.win_id(*k));
                }
            }
        }
        let due = self
            .wm
            .last_save
            .map(|t| t.elapsed().map(|e| e.as_secs() > 20).unwrap_or(true))
            .unwrap_or(true);
        if due {
            self.wm.save();
            self.wm.last_save = Some(std::time::SystemTime::now());
        }
    }

    pub(super) fn wm_shortcuts(&mut self, ctx: &egui::Context) {
        let (mods, keys) = ctx.input(|i| (i.modifiers, i.keys_down.clone()));
        if !(mods.command && mods.alt) {
            return;
        }
        let pressed = |k: egui::Key| ctx.input(|i| i.key_pressed(k));
        let digits = [
            egui::Key::Num1,
            egui::Key::Num2,
            egui::Key::Num3,
            egui::Key::Num4,
        ];
        for (n, k) in digits.iter().enumerate() {
            if pressed(*k) {
                if mods.shift {
                    if let Some(f) = self.wm.focused {
                        self.move_to_desktop(f, n as u8);
                        self.notify(format!("Moved {} to desktop {}", f.label(), n + 1));
                    }
                } else {
                    self.wm.desktop = n as u8;
                }
            }
        }
        let _ = keys;
        let Some(f) = self.wm.focused else { return };
        if pressed(egui::Key::ArrowLeft) {
            self.snap(
                f,
                if mods.shift {
                    Zone::TopLeft
                } else {
                    Zone::Left
                },
            );
        } else if pressed(egui::Key::ArrowRight) {
            self.snap(
                f,
                if mods.shift {
                    Zone::TopRight
                } else {
                    Zone::Right
                },
            );
        } else if pressed(egui::Key::ArrowUp) {
            self.snap(f, Zone::Max);
        } else if pressed(egui::Key::ArrowDown) {
            self.restore(f);
        } else if pressed(egui::Key::M) {
            self.wm_minimize(f);
        }
    }

    /// Dock button for a window, with a right-click window menu.
    pub(super) fn dock_item(&mut self, ui: &mut egui::Ui, key: WinId) {
        let open = self.is_open(key);
        let desk = self.wm.desktop;
        let (minimized, elsewhere) = self
            .wm
            .peek(key)
            .map(|s| (s.minimized, s.desktop != desk))
            .unwrap_or((false, false));
        let label = key.label();
        let text = if open && !minimized && !elsewhere {
            RichText::new(label).strong().color(theme::accent())
        } else if open {
            RichText::new(format!("{label} •")).color(theme::DIM)
        } else {
            RichText::new(label)
        };
        let resp = ui
            .add_sized([92.0, 32.0], egui::Button::new(text))
            .on_hover_text(if open && minimized {
                "Minimised: click to restore. Right-click for window options"
            } else if open && elsewhere {
                "On another desktop: click to bring it here"
            } else {
                "Right-click for window options"
            });
        if resp.clicked() {
            if !open {
                self.open_win(key);
                self.wm_show(key);
            } else if minimized || elsewhere {
                self.wm_show(key);
            } else {
                self.wm_minimize(key);
            }
        }
        resp.context_menu(|ui| {
            if open {
                if minimized {
                    if ui.button("Restore").clicked() {
                        self.wm_show(key);
                        ui.close_menu();
                    }
                } else if ui.button("Minimise").clicked() {
                    self.wm_minimize(key);
                    ui.close_menu();
                }
                if ui.button("Maximise").clicked() {
                    self.snap(key, Zone::Max);
                    ui.close_menu();
                }
                if ui.button("Snap left").clicked() {
                    self.snap(key, Zone::Left);
                    ui.close_menu();
                }
                if ui.button("Snap right").clicked() {
                    self.snap(key, Zone::Right);
                    ui.close_menu();
                }
                ui.menu_button("Snap to quarter", |ui| {
                    for (z, n) in [
                        (Zone::TopLeft, "Top left"),
                        (Zone::TopRight, "Top right"),
                        (Zone::BottomLeft, "Bottom left"),
                        (Zone::BottomRight, "Bottom right"),
                    ] {
                        if ui.button(n).clicked() {
                            self.snap(key, z);
                            ui.close_menu();
                        }
                    }
                });
                if ui.button("Restore size").clicked() {
                    self.restore(key);
                    ui.close_menu();
                }
                ui.menu_button("Move to desktop", |ui| {
                    for d in 0..4u8 {
                        if ui.button(format!("Desktop {}", d + 1)).clicked() {
                            self.move_to_desktop(key, d);
                            ui.close_menu();
                        }
                    }
                });
                ui.separator();
                if ui.button("Close").clicked() {
                    *self.win_flag(key) = false;
                    ui.close_menu();
                }
            } else if ui.button("Open").clicked() {
                self.open_win(key);
                self.wm_show(key);
                ui.close_menu();
            }
        });
    }

    /// "1 2 3 4" desktop switcher for the top bar.
    pub(super) fn desktop_switcher(&mut self, ui: &mut egui::Ui) {
        for d in (0..4u8).rev() {
            let busy = MANAGED.iter().any(|k| {
                self.wm.peek(*k).map(|s| s.desktop == d).unwrap_or(false) && {
                    let k = *k;
                    matches!(k, _ if self.win_open_peek(k))
                }
            });
            let text = if busy {
                format!("{}•", d + 1)
            } else {
                format!("{}", d + 1)
            };
            if ui
                .selectable_label(self.wm.desktop == d, text)
                .on_hover_text(format!("Desktop {} (Ctrl+Alt+{})", d + 1, d + 1))
                .clicked()
            {
                self.wm.desktop = d;
            }
        }
    }

    fn win_open_peek(&self, key: WinId) -> bool {
        match key {
            WinId::Files => self.win.files,
            WinId::Terminal => self.win.terminal,
            WinId::Claude => self.win.claude,
            WinId::Shield => self.win.shield,
            WinId::Media => self.win.media,
            WinId::Office => self.win.office,
            WinId::Store => self.win.store,
            WinId::Monitor => self.win.monitor,
            WinId::Settings => self.win.settings,
        }
    }
}
