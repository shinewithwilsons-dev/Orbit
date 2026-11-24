//! Colours and wallpaper.

use eframe::egui::{self, Color32, Pos2, Rect, Rounding, Stroke};

pub const ACCENT: Color32 = Color32::from_rgb(124, 140, 255);
pub const CLAUDE: Color32 = Color32::from_rgb(217, 119, 87);
pub const OK: Color32 = Color32::from_rgb(110, 210, 150);
pub const WARN: Color32 = Color32::from_rgb(240, 180, 90);
pub const BAD: Color32 = Color32::from_rgb(240, 100, 100);
pub const DIM: Color32 = Color32::from_rgb(150, 155, 175);

pub fn apply(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.window_rounding = Rounding::same(10.0_f32);
    v.window_fill = Color32::from_rgba_unmultiplied(22, 24, 36, 245);
    v.panel_fill = Color32::from_rgb(16, 17, 27);
    v.window_stroke = Stroke::new(1.0_f32, Color32::from_rgb(55, 60, 90));
    v.selection.bg_fill = ACCENT.linear_multiply(0.5);
    v.hyperlink_color = ACCENT;
    v.widgets.active.bg_fill = ACCENT;
    ctx.set_visuals(v);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    ctx.set_style(style);
}

/// Colour for a 0..1 load value.
pub fn load_colour(f: f32) -> Color32 {
    if f < 0.6 {
        OK
    } else if f < 0.85 {
        WARN
    } else {
        BAD
    }
}

/// Deep-space wallpaper: dark base, two nebula glows and a fixed starfield.
pub fn paint_wallpaper(painter: &egui::Painter, rect: Rect, t: f64) {
    painter.rect_filled(rect, 0.0, Color32::from_rgb(8, 9, 20));

    let glow = |c: Pos2, r: f32, col: (u8, u8, u8)| {
        // Many faint layers, largest first, give a smooth falloff.
        let steps = 60;
        for i in (0..steps).rev() {
            let k = (i + 1) as f32 / steps as f32;
            painter.circle_filled(
                c,
                r * k,
                Color32::from_rgba_unmultiplied(col.0, col.1, col.2, 2),
            );
        }
    };
    glow(
        rect.lerp_inside(egui::vec2(0.25, 0.3)),
        rect.width() * 0.35,
        (90, 70, 200),
    );
    glow(
        rect.lerp_inside(egui::vec2(0.8, 0.75)),
        rect.width() * 0.3,
        (200, 90, 120),
    );

    // Deterministic starfield with a gentle twinkle.
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % 10_000) as f32 / 10_000.0
    };
    for i in 0..260 {
        let x = rect.left() + next() * rect.width();
        let y = rect.top() + next() * rect.height();
        let size = 0.4 + next() * 1.3;
        let phase = next() * 6.28;
        let tw = (0.55 + 0.45 * ((t as f32) * 0.8 + phase + i as f32).sin()).clamp(0.1, 1.0);
        let a = (tw * 220.0) as u8;
        painter.circle_filled(
            Pos2::new(x, y),
            size,
            Color32::from_rgba_unmultiplied(230, 235, 255, a),
        );
    }
}
