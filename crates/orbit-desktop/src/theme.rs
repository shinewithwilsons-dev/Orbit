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

// ---------------------------------------------------------------- icons --

/// Icon artwork, drawn with shapes so it looks the same on every OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Folder,
    File,
    Terminal,
    Claude,
    Shield,
    Media,
    Office,
    Monitor,
    Settings,
    Apps,
}

pub const SHIELD_GREEN: Color32 = Color32::from_rgb(70, 190, 130);
pub const MEDIA_PINK: Color32 = Color32::from_rgb(230, 90, 150);
pub const OFFICE_BLUE: Color32 = Color32::from_rgb(70, 140, 230);
pub const FOLDER_GOLD: Color32 = Color32::from_rgb(240, 190, 80);

impl Glyph {
    pub fn colour(self) -> Color32 {
        match self {
            Glyph::Folder => FOLDER_GOLD,
            Glyph::File => Color32::from_rgb(200, 205, 220),
            Glyph::Terminal => Color32::from_rgb(60, 66, 90),
            Glyph::Claude => CLAUDE,
            Glyph::Shield => SHIELD_GREEN,
            Glyph::Media => MEDIA_PINK,
            Glyph::Office => OFFICE_BLUE,
            Glyph::Monitor => Color32::from_rgb(120, 110, 220),
            Glyph::Settings => Color32::from_rgb(120, 128, 150),
            Glyph::Apps => ACCENT,
        }
    }
}

/// Draw an icon filling `rect` (square). `label` is a small emblem
/// (folder letter, file extension).
pub fn paint_icon(p: &egui::Painter, rect: Rect, glyph: Glyph, label: &str) {
    let c = rect.center();
    let s = rect.width();
    let white = Color32::from_white_alpha(235);
    let stroke = Stroke::new((s * 0.06).max(1.5), white);
    match glyph {
        Glyph::Folder => {
            let tab = Rect::from_min_size(
                rect.min + egui::vec2(s * 0.08, s * 0.16),
                egui::vec2(s * 0.38, s * 0.16),
            );
            p.rect_filled(
                tab,
                Rounding::same(s * 0.05),
                FOLDER_GOLD.linear_multiply(0.8),
            );
            let body = Rect::from_min_max(
                rect.min + egui::vec2(s * 0.06, s * 0.26),
                rect.max - egui::vec2(s * 0.06, s * 0.12),
            );
            p.rect_filled(body, Rounding::same(s * 0.08), FOLDER_GOLD);
            p.rect_filled(
                Rect::from_min_max(body.min, egui::pos2(body.max.x, body.min.y + s * 0.08)),
                Rounding::same(s * 0.06),
                Color32::from_rgb(250, 215, 120),
            );
            if !label.is_empty() {
                p.text(
                    body.center() + egui::vec2(0.0, s * 0.04),
                    egui::Align2::CENTER_CENTER,
                    label,
                    egui::FontId::proportional(s * 0.24),
                    Color32::from_rgb(120, 80, 20),
                );
            }
        }
        Glyph::File => {
            let page = Rect::from_center_size(c, egui::vec2(s * 0.62, s * 0.8));
            p.rect_filled(page, Rounding::same(s * 0.06), Glyph::File.colour());
            let fold = s * 0.18;
            p.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(page.max.x - fold, page.min.y),
                    egui::pos2(page.max.x, page.min.y + fold),
                    egui::pos2(page.max.x - fold, page.min.y + fold),
                ],
                Color32::from_rgb(150, 155, 175),
                Stroke::NONE,
            ));
            let tag = Rect::from_center_size(
                egui::pos2(c.x, page.max.y - s * 0.2),
                egui::vec2(s * 0.56, s * 0.2),
            );
            p.rect_filled(tag, Rounding::same(s * 0.04), file_colour(label));
            p.text(
                tag.center(),
                egui::Align2::CENTER_CENTER,
                label.to_uppercase(),
                egui::FontId::proportional(s * 0.13),
                Color32::WHITE,
            );
        }
        _ => {
            // App tile: rounded square with a highlight band.
            let tile = rect.shrink(s * 0.06);
            p.rect_filled(tile, Rounding::same(s * 0.22), glyph.colour());
            p.rect_filled(
                Rect::from_min_max(tile.min, egui::pos2(tile.max.x, tile.center().y)),
                Rounding {
                    nw: s * 0.22,
                    ne: s * 0.22,
                    sw: 0.0,
                    se: 0.0,
                },
                Color32::from_white_alpha(22),
            );
            let r = s * 0.24;
            match glyph {
                Glyph::Terminal => {
                    p.line_segment(
                        [c + egui::vec2(-r, -r * 0.6), c + egui::vec2(-r * 0.2, 0.0)],
                        stroke,
                    );
                    p.line_segment(
                        [c + egui::vec2(-r * 0.2, 0.0), c + egui::vec2(-r, r * 0.6)],
                        stroke,
                    );
                    p.line_segment(
                        [c + egui::vec2(0.1 * r, r * 0.7), c + egui::vec2(r, r * 0.7)],
                        stroke,
                    );
                }
                Glyph::Claude => {
                    // Radiating burst.
                    for i in 0..8 {
                        let a = i as f32 * std::f32::consts::PI / 4.0;
                        let d = egui::vec2(a.cos(), a.sin());
                        p.line_segment([c + d * r * 0.25, c + d * r * 1.1], stroke);
                    }
                }
                Glyph::Shield => {
                    let pts = vec![
                        c + egui::vec2(0.0, -r * 1.15),
                        c + egui::vec2(r, -r * 0.7),
                        c + egui::vec2(r * 0.85, r * 0.3),
                        c + egui::vec2(0.0, r * 1.15),
                        c + egui::vec2(-r * 0.85, r * 0.3),
                        c + egui::vec2(-r, -r * 0.7),
                    ];
                    p.add(egui::Shape::convex_polygon(
                        pts,
                        Color32::from_white_alpha(40),
                        stroke,
                    ));
                    // Tick.
                    p.line_segment(
                        [
                            c + egui::vec2(-r * 0.45, 0.0),
                            c + egui::vec2(-r * 0.1, r * 0.4),
                        ],
                        stroke,
                    );
                    p.line_segment(
                        [
                            c + egui::vec2(-r * 0.1, r * 0.4),
                            c + egui::vec2(r * 0.5, -r * 0.35),
                        ],
                        stroke,
                    );
                }
                Glyph::Media => {
                    p.add(egui::Shape::convex_polygon(
                        vec![
                            c + egui::vec2(-r * 0.6, -r),
                            c + egui::vec2(r, 0.0),
                            c + egui::vec2(-r * 0.6, r),
                        ],
                        white,
                        Stroke::NONE,
                    ));
                }
                Glyph::Office => {
                    let page = Rect::from_center_size(c, egui::vec2(r * 1.5, r * 2.0));
                    p.rect_stroke(page, Rounding::same(s * 0.03), stroke);
                    for k in 0..3 {
                        let y = page.min.y + r * (0.55 + 0.45 * k as f32);
                        p.line_segment(
                            [
                                egui::pos2(page.min.x + r * 0.3, y),
                                egui::pos2(page.max.x - r * 0.3, y),
                            ],
                            stroke,
                        );
                    }
                }
                Glyph::Monitor => {
                    for (k, h) in [0.6f32, 1.2, 0.9, 1.6].iter().enumerate() {
                        let x = c.x - r + k as f32 * r * 0.62;
                        let bar = Rect::from_min_max(
                            egui::pos2(x, c.y + r - r * h),
                            egui::pos2(x + r * 0.4, c.y + r),
                        );
                        p.rect_filled(bar, Rounding::same(2.0), white);
                    }
                }
                Glyph::Settings => {
                    p.circle_stroke(c, r * 0.55, stroke);
                    for i in 0..8 {
                        let a = i as f32 * std::f32::consts::PI / 4.0;
                        let d = egui::vec2(a.cos(), a.sin());
                        p.line_segment([c + d * r * 0.75, c + d * r * 1.05], stroke);
                    }
                }
                Glyph::Apps => {
                    for i in 0..3 {
                        for j in 0..3 {
                            let o = egui::vec2(
                                (i as f32 - 1.0) * r * 0.75,
                                (j as f32 - 1.0) * r * 0.75,
                            );
                            p.circle_filled(c + o, r * 0.18, white);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

/// Tag colour for a file extension.
pub fn file_colour(ext: &str) -> Color32 {
    let e = ext.to_lowercase();
    if orbit_core::media::AUDIO_EXT.contains(&e.as_str())
        || orbit_core::media::VIDEO_EXT.contains(&e.as_str())
    {
        MEDIA_PINK
    } else if ["doc", "docx", "odt", "rtf", "txt", "md", "pdf"].contains(&e.as_str()) {
        OFFICE_BLUE
    } else if ["xls", "xlsx", "ods", "csv"].contains(&e.as_str()) {
        SHIELD_GREEN
    } else if ["ppt", "pptx", "odp"].contains(&e.as_str()) {
        WARN
    } else if ["exe", "msi", "bat", "cmd", "ps1", "sh", "app", "dmg", "deb"].contains(&e.as_str()) {
        BAD
    } else if ["png", "jpg", "jpeg", "gif", "webp", "svg"].contains(&e.as_str()) {
        Color32::from_rgb(150, 110, 230)
    } else {
        Color32::from_rgb(110, 115, 140)
    }
}
