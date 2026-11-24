//! Themes, accent colours and wallpapers.

use eframe::egui::{self, Color32, Pos2, Rect, Rounding, Stroke};
use std::sync::atomic::{AtomicU32, Ordering};

pub const CLAUDE: Color32 = Color32::from_rgb(217, 119, 87);
pub const OK: Color32 = Color32::from_rgb(110, 210, 150);
pub const WARN: Color32 = Color32::from_rgb(240, 180, 90);
pub const BAD: Color32 = Color32::from_rgb(240, 100, 100);
pub const DIM: Color32 = Color32::from_rgb(150, 155, 175);

static ACCENT_RGB: AtomicU32 = AtomicU32::new(0x7C8CFF);

/// The current accent colour (set by the theme or the user).
pub fn accent() -> Color32 {
    let v = ACCENT_RGB.load(Ordering::Relaxed);
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

pub struct ThemeDef {
    pub key: &'static str,
    pub name: &'static str,
    pub dark: bool,
    pub accent: [u8; 3],
    pub panel: [u8; 3],
    pub window: [u8; 3],
    pub sky: [u8; 3],
    pub glow1: [u8; 3],
    pub glow2: [u8; 3],
}

pub const THEMES: &[ThemeDef] = &[
    ThemeDef {
        key: "orbit",
        name: "Orbit",
        dark: true,
        accent: [124, 140, 255],
        panel: [16, 17, 27],
        window: [22, 24, 36],
        sky: [8, 9, 20],
        glow1: [90, 70, 200],
        glow2: [200, 90, 120],
    },
    ThemeDef {
        key: "mars",
        name: "Mars",
        dark: true,
        accent: [240, 120, 70],
        panel: [24, 14, 12],
        window: [34, 20, 18],
        sky: [20, 8, 6],
        glow1: [200, 70, 40],
        glow2: [230, 150, 60],
    },
    ThemeDef {
        key: "ocean",
        name: "Deep Ocean",
        dark: true,
        accent: [60, 200, 210],
        panel: [8, 20, 28],
        window: [12, 28, 38],
        sky: [2, 10, 20],
        glow1: [20, 120, 160],
        glow2: [40, 200, 170],
    },
    ThemeDef {
        key: "mono",
        name: "Mono",
        dark: true,
        accent: [200, 200, 210],
        panel: [18, 18, 20],
        window: [26, 26, 28],
        sky: [10, 10, 12],
        glow1: [80, 80, 90],
        glow2: [120, 120, 130],
    },
    ThemeDef {
        key: "light",
        name: "Daylight",
        dark: false,
        accent: [60, 90, 220],
        panel: [236, 238, 245],
        window: [248, 249, 252],
        sky: [170, 200, 240],
        glow1: [255, 255, 255],
        glow2: [250, 200, 220],
    },
];

pub fn theme_def(key: &str) -> &'static ThemeDef {
    THEMES.iter().find(|t| t.key == key).unwrap_or(&THEMES[0])
}

pub const WALLPAPERS: &[(&str, &str)] = &[
    ("starfield", "Starfield"),
    ("warp", "Warp speed"),
    ("planet", "Planet & moon"),
    ("aurora", "Aurora"),
    ("plain", "Plain"),
    ("image", "Your picture"),
];

fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

pub fn apply(ctx: &egui::Context, cfg: &orbit_core::Config) {
    let t = theme_def(&cfg.theme);
    let a = cfg.accent.unwrap_or(t.accent);
    ACCENT_RGB.store(
        ((a[0] as u32) << 16) | ((a[1] as u32) << 8) | a[2] as u32,
        Ordering::Relaxed,
    );
    let accent = rgb(a);
    let mut v = if t.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.window_rounding = Rounding::same(10.0_f32);
    v.window_fill = Color32::from_rgba_unmultiplied(t.window[0], t.window[1], t.window[2], 248);
    v.panel_fill = rgb(t.panel);
    v.window_stroke = Stroke::new(1.0_f32, accent.linear_multiply(0.35));
    v.selection.bg_fill = accent.linear_multiply(0.5);
    v.hyperlink_color = accent;
    v.widgets.active.bg_fill = accent;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, accent);
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

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % 10_000) as f32 / 10_000.0
    }
}

fn glow(painter: &egui::Painter, c: Pos2, r: f32, col: [u8; 3], strength: u8) {
    // Many faint layers, largest first, give a smooth falloff.
    let steps = 60;
    for i in (0..steps).rev() {
        let k = (i + 1) as f32 / steps as f32;
        painter.circle_filled(
            c,
            r * k,
            Color32::from_rgba_unmultiplied(col[0], col[1], col[2], strength),
        );
    }
}

fn stars(painter: &egui::Painter, rect: Rect, t: f32, count: usize, colour: [u8; 3]) {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for i in 0..count {
        let x = rect.left() + rng.next() * rect.width();
        let y = rect.top() + rng.next() * rect.height();
        let size = 0.4 + rng.next() * 1.3;
        let phase = rng.next() * 6.28;
        let tw = (0.55 + 0.45 * (t * 0.8 + phase + i as f32).sin()).clamp(0.1, 1.0);
        painter.circle_filled(
            Pos2::new(x, y),
            size,
            Color32::from_rgba_unmultiplied(colour[0], colour[1], colour[2], (tw * 220.0) as u8),
        );
    }
}

/// Paint the desktop wallpaper. `image` is the user's picture, if any.
pub fn paint_wallpaper(
    painter: &egui::Painter,
    rect: Rect,
    t: f64,
    cfg: &orbit_core::Config,
    image: Option<&egui::TextureHandle>,
) {
    let th = theme_def(&cfg.theme);
    let t = if cfg.animate_wallpaper { t as f32 } else { 0.0 };
    painter.rect_filled(rect, 0.0, rgb(th.sky));
    let star_col = if th.dark {
        [230, 235, 255]
    } else {
        [255, 255, 255]
    };
    match cfg.wallpaper.as_str() {
        "image" if image.is_some() => {
            let tex = image.unwrap();
            // Cover the screen, keeping the picture's shape.
            let size = tex.size_vec2();
            let scale = (rect.width() / size.x).max(rect.height() / size.y);
            let draw = Rect::from_center_size(rect.center(), size * scale);
            painter.image(
                tex.id(),
                draw,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        "warp" => {
            let c = rect.center();
            let mut rng = Rng(42);
            let reach = rect.width().max(rect.height()) * 0.75;
            for _ in 0..220 {
                let angle = rng.next() * std::f32::consts::TAU;
                let speed = 0.15 + rng.next() * 0.35;
                let offset = rng.next();
                let d = ((offset + t * speed * 0.25) % 1.0).powf(2.2);
                let dir = egui::vec2(angle.cos(), angle.sin());
                let start = c + dir * (d * reach);
                let end = c + dir * ((d + 0.02 + d * 0.12) * reach);
                let alpha = (d * 255.0).min(230.0) as u8;
                painter.line_segment(
                    [start, end],
                    Stroke::new(
                        0.6 + d * 2.2,
                        Color32::from_rgba_unmultiplied(200, 215, 255, alpha),
                    ),
                );
            }
            glow(painter, c, rect.width() * 0.18, th.glow1, 3);
        }
        "planet" => {
            stars(painter, rect, t, 220, star_col);
            let c = rect.lerp_inside(egui::vec2(0.7, 0.6));
            let r = rect.height() * 0.26;
            glow(painter, c, r * 1.7, th.glow1, 2);
            let ring = |front: bool| -> Vec<Pos2> {
                (0..=90)
                    .map(|i| {
                        let a = if front {
                            i as f32 / 90.0 * std::f32::consts::PI
                        } else {
                            std::f32::consts::PI + i as f32 / 90.0 * std::f32::consts::PI
                        };
                        c + egui::vec2(a.cos() * r * 1.8, a.sin() * r * 0.34)
                    })
                    .collect()
            };
            let ring_col =
                Color32::from_rgba_unmultiplied(th.glow2[0], th.glow2[1], th.glow2[2], 170);
            // Back half of the ring, behind the planet.
            painter.add(egui::Shape::line(
                ring(false),
                Stroke::new(4.0_f32, ring_col),
            ));
            // Planet: smooth radial shading (lit from the top left) as a triangle fan.
            let base = th.glow1;
            let lit = Color32::from_rgb(
                base[0].saturating_add(70),
                base[1].saturating_add(70),
                base[2].saturating_add(70),
            );
            let dark = [
                base[0] as f32 / 5.0,
                base[1] as f32 / 5.0,
                base[2] as f32 / 4.0,
            ];
            let mut mesh = egui::Mesh::default();
            mesh.colored_vertex(c - egui::vec2(r * 0.38, r * 0.42), lit);
            let n = 96u32;
            for i in 0..=n {
                let a = i as f32 / n as f32 * std::f32::consts::TAU;
                // Edges facing the light stay a little brighter.
                let facing = ((a - 3.93).cos() * 0.5 + 0.5).powf(2.0);
                let mix = |d: f32, b: u8| (d + (b as f32 * 0.6 - d) * facing) as u8;
                let col = Color32::from_rgb(
                    mix(dark[0], base[0]),
                    mix(dark[1], base[1]),
                    mix(dark[2], base[2]),
                );
                mesh.colored_vertex(c + egui::vec2(a.cos() * r, a.sin() * r), col);
                if i > 0 {
                    mesh.add_triangle(0, i, i + 1);
                }
            }
            painter.add(egui::Shape::mesh(mesh));
            // Front half of the ring.
            painter.add(egui::Shape::line(
                ring(true),
                Stroke::new(4.0_f32, ring_col),
            ));
            // Moon on a slow orbit.
            let a = t * 0.12;
            let moon = c + egui::vec2(a.cos() * r * 2.4, a.sin() * r * 0.8);
            painter.circle_filled(moon, r * 0.09, Color32::from_rgb(200, 200, 212));
            painter.circle_filled(
                moon + egui::vec2(r * 0.025, r * 0.02),
                r * 0.075,
                Color32::from_rgb(150, 150, 165),
            );
        }
        "aurora" => {
            stars(painter, rect, t, 160, star_col);
            for band in 0..4 {
                let col = if band % 2 == 0 { th.glow2 } else { th.glow1 };
                let base_y = rect.top() + rect.height() * (0.25 + band as f32 * 0.09);
                for k in 0..14 {
                    let pts: Vec<Pos2> = (0..=80)
                        .map(|i| {
                            let x = rect.left() + rect.width() * i as f32 / 80.0;
                            let y = base_y
                                + (x * 0.004 + t * 0.4 + band as f32).sin() * 40.0
                                + (x * 0.011 - t * 0.25).sin() * 18.0
                                + k as f32 * 4.0;
                            Pos2::new(x, y)
                        })
                        .collect();
                    painter.add(egui::Shape::line(
                        pts,
                        Stroke::new(
                            5.0_f32,
                            Color32::from_rgba_unmultiplied(col[0], col[1], col[2], 7),
                        ),
                    ));
                }
            }
        }
        "plain" => {
            glow(
                painter,
                rect.lerp_inside(egui::vec2(0.5, 1.1)),
                rect.width() * 0.6,
                th.glow1,
                2,
            );
        }
        _ => {
            glow(
                painter,
                rect.lerp_inside(egui::vec2(0.25, 0.3)),
                rect.width() * 0.35,
                th.glow1,
                2,
            );
            glow(
                painter,
                rect.lerp_inside(egui::vec2(0.8, 0.75)),
                rect.width() * 0.3,
                th.glow2,
                2,
            );
            stars(painter, rect, t, 260, star_col);
        }
    }
}

/// Wallpapers that move.
pub fn is_animated(wallpaper: &str) -> bool {
    matches!(wallpaper, "starfield" | "warp" | "planet" | "aurora")
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
            Glyph::Apps => accent(),
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
