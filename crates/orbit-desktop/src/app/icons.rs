//! Clickable desktop icons: click to select, double-click (or Enter) to open.

use super::*;
use crate::theme::{paint_icon, Glyph};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WinId {
    Files,
    Terminal,
    Claude,
    Shield,
    Media,
    Office,
    Monitor,
    Settings,
}

#[derive(Debug, Clone)]
pub(super) enum IconAction {
    Win(WinId),
    Folder(PathBuf),
    Launcher,
}

#[derive(Debug, Clone)]
pub(super) struct DesktopIcon {
    pub label: String,
    pub glyph: Glyph,
    pub emblem: String,
    pub action: IconAction,
}

pub(super) fn default_icons() -> Vec<DesktopIcon> {
    let mut v = Vec::new();
    let mut folder = |label: &str, emblem: &str, path: Option<PathBuf>| {
        if let Some(p) = path.filter(|p| p.exists()) {
            v.push(DesktopIcon {
                label: label.into(),
                glyph: Glyph::Folder,
                emblem: emblem.into(),
                action: IconAction::Folder(p),
            });
        }
    };
    folder("Home", "H", dirs_home());
    folder("Documents", "D", orbit_core_dirs::documents());
    folder("Downloads", "DL", orbit_core_dirs::downloads());
    folder("Music", "M", orbit_core_dirs::music());
    folder("Videos", "V", orbit_core_dirs::videos());
    let app = |label: &str, glyph: Glyph, w: WinId| DesktopIcon {
        label: label.into(),
        glyph,
        emblem: String::new(),
        action: IconAction::Win(w),
    };
    v.push(app("Files", Glyph::Folder, WinId::Files));
    v.push(app("Terminal", Glyph::Terminal, WinId::Terminal));
    v.push(app("Claude", Glyph::Claude, WinId::Claude));
    v.push(app("Orbit Shield", Glyph::Shield, WinId::Shield));
    v.push(app("Media", Glyph::Media, WinId::Media));
    v.push(app("Office", Glyph::Office, WinId::Office));
    v.push(app("Monitor", Glyph::Monitor, WinId::Monitor));
    v.push(app("Settings", Glyph::Settings, WinId::Settings));
    v.push(DesktopIcon {
        label: "Apps".into(),
        glyph: Glyph::Apps,
        emblem: String::new(),
        action: IconAction::Launcher,
    });
    v
}

pub(super) fn dirs_home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// Standard user folders (Documents, Downloads, …), localised per OS.
pub(super) mod orbit_core_dirs {
    use std::path::PathBuf;
    pub fn documents() -> Option<PathBuf> {
        orbit_core::user_dirs::documents()
    }
    pub fn downloads() -> Option<PathBuf> {
        orbit_core::user_dirs::downloads()
    }
    pub fn music() -> Option<PathBuf> {
        orbit_core::user_dirs::music()
    }
    pub fn videos() -> Option<PathBuf> {
        orbit_core::user_dirs::videos()
    }
}

const CELL: egui::Vec2 = egui::vec2(96.0, 98.0);
const ICON: f32 = 54.0;

impl Desktop {
    pub(super) fn open_win(&mut self, w: WinId) {
        match w {
            WinId::Files => self.win.files = true,
            WinId::Terminal => self.win.terminal = true,
            WinId::Claude => self.win.claude = true,
            WinId::Shield => self.win.shield = true,
            WinId::Media => self.open_media(),
            WinId::Office => self.open_office(),
            WinId::Monitor => self.win.monitor = true,
            WinId::Settings => self.win.settings = true,
        }
    }

    pub(super) fn run_icon(&mut self, action: IconAction) {
        match action {
            IconAction::Win(w) => self.open_win(w),
            IconAction::Folder(p) => {
                self.win.files = true;
                self.go_to(p);
            }
            IconAction::Launcher => self.show_launcher = true,
        }
    }

    /// Draw the icon grid down the left of the desktop.
    pub(super) fn desktop_icons(&mut self, ui: &mut egui::Ui, area: egui::Rect) {
        // Clicking empty desktop clears the selection.
        let bg = ui.interact(area, egui::Id::new("desktop_bg"), egui::Sense::click());
        if bg.clicked() {
            self.selected_icon = None;
        }
        let rows = ((area.height() - 16.0) / CELL.y).floor().max(1.0) as usize;
        let mut open: Option<usize> = None;
        let painter = ui.painter().clone();
        for (i, icon) in self.icons.iter().enumerate() {
            let col = i / rows;
            let row = i % rows;
            let min = area.min + egui::vec2(14.0 + col as f32 * CELL.x, 12.0 + row as f32 * CELL.y);
            let cell = egui::Rect::from_min_size(min, CELL);
            let resp = ui.interact(
                cell,
                egui::Id::new(("desktop_icon", i)),
                egui::Sense::click(),
            );
            let selected = self.selected_icon == Some(i);
            if selected || resp.hovered() {
                painter.rect_filled(
                    cell.shrink(3.0),
                    egui::Rounding::same(10.0),
                    Color32::from_rgba_unmultiplied(140, 160, 255, if selected { 70 } else { 30 }),
                );
            }
            let icon_rect = egui::Rect::from_center_size(
                egui::pos2(cell.center().x, cell.min.y + 8.0 + ICON / 2.0),
                egui::vec2(ICON, ICON),
            );
            paint_icon(&painter, icon_rect, icon.glyph, &icon.emblem);
            let text_pos = egui::pos2(cell.center().x, icon_rect.max.y + 13.0);
            let font = egui::FontId::proportional(13.0);
            painter.text(
                text_pos + egui::vec2(1.0, 1.0),
                egui::Align2::CENTER_CENTER,
                &icon.label,
                font.clone(),
                Color32::from_black_alpha(200),
            );
            painter.text(
                text_pos,
                egui::Align2::CENTER_CENTER,
                &icon.label,
                font,
                Color32::WHITE,
            );
            if resp.clicked() {
                self.selected_icon = Some(i);
            }
            if resp.double_clicked() {
                open = Some(i);
            }
        }
        // Enter opens the selected icon (when no text box has focus).
        if open.is_none()
            && !ui.ctx().wants_keyboard_input()
            && ui.input(|i| i.key_pressed(egui::Key::Enter))
        {
            open = self.selected_icon;
        }
        if let Some(i) = open {
            let action = self.icons[i].action.clone();
            self.run_icon(action);
        }
    }
}
