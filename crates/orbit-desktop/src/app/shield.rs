//! Orbit Shield window: a dashboard with the protection status at its
//! centre, scans, malware removal tools, quarantine, network and settings.
//! Every finding has "Ask Claude".

use super::*;
use egui::{pos2, vec2, Pos2, Rect, Sense, Stroke};
use orbit_core::antivirus::{
    self, Finding, QuarantineItem, ScanOptions, ScanReport, Severity, Watcher,
};
use orbit_core::malware::{self, DeepTool, DisabledItem, Sweep};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

// Palette taken from the Orbit Shield design: slate panels, one blue.
const BG: Color32 = Color32::from_rgb(27, 33, 42);
const SIDEBAR: Color32 = Color32::from_rgb(22, 27, 35);
const CARD: Color32 = Color32::from_rgb(35, 43, 55);
const CARD_HI: Color32 = Color32::from_rgb(44, 54, 69);
const LINE: Color32 = Color32::from_rgb(52, 63, 79);
const BLUE: Color32 = Color32::from_rgb(58, 156, 226);
const TEXT: Color32 = Color32::from_rgb(232, 238, 246);
const MUTED: Color32 = Color32::from_rgb(138, 152, 170);
const GREEN: Color32 = Color32::from_rgb(76, 200, 140);

#[derive(PartialEq, Eq, Clone, Copy)]
pub(super) enum ShieldTab {
    Dashboard,
    Scan,
    Removal,
    Quarantine,
    Network,
    Protection,
}

#[derive(Clone, Copy)]
enum Sym {
    Grid,
    Search,
    Bug,
    Box,
    Globe,
    Gear,
    Bolt,
    Data,
    Clock,
}

#[allow(dead_code)] // watch/schedule fields are kept for the polling fallback
pub(super) struct ShieldState {
    pub tab: ShieldTab,
    pub scanning: bool,
    pub count: usize,
    pub current: String,
    pub cancel: Arc<AtomicBool>,
    pub findings: Vec<Finding>,
    pub summary: Option<String>,
    pub notes: Vec<String>,
    pub quarantine: Vec<QuarantineItem>,
    pub custom: String,
    pub watcher: Option<Watcher>,
    pub last_poll: Instant,
    pub clamav: Option<String>,
    pub signatures: usize,
    pub updating: bool,
    pub confirm_delete: Option<QuarantineItem>,
    pub live: Option<orbit_core::watch::LiveWatcher>,
    pub last_schedule_check: Instant,
    pub net: Vec<orbit_core::network::Connection>,
    pub net_loading: bool,
    pub net_err: Option<String>,
    pub net_filter: String,
    pub yara: bool,
    // Malware removal.
    pub sweep: Option<Sweep>,
    pub sweeping: bool,
    pub disabled: Vec<DisabledItem>,
    pub deep: Vec<DeepTool>,
    pub confirm_temp: bool,
    pub confirm_remove_all: bool,
    pub show_all_startup: bool,
    pub cleaning: bool,
}

impl ShieldState {
    pub fn new(watch: bool) -> Self {
        Self {
            tab: ShieldTab::Dashboard,
            scanning: false,
            count: 0,
            current: String::new(),
            cancel: Arc::new(AtomicBool::new(false)),
            findings: Vec::new(),
            summary: None,
            notes: Vec::new(),
            quarantine: antivirus::quarantine_list(),
            custom: String::new(),
            watcher: watch.then(|| Watcher::new(antivirus::watch_paths())),
            last_poll: Instant::now(),
            clamav: antivirus::clamav_version(),
            signatures: antivirus::Engines::load().signature_count(),
            updating: false,
            confirm_delete: None,
            live: None,
            last_schedule_check: Instant::now() - Duration::from_secs(3600),
            net: Vec::new(),
            net_loading: false,
            net_err: None,
            net_filter: String::new(),
            yara: antivirus::find_yara().is_some(),
            sweep: None,
            sweeping: false,
            disabled: malware::disabled_list(),
            deep: malware::deep_tools(),
            confirm_temp: false,
            confirm_remove_all: false,
            show_all_startup: false,
            cleaning: false,
        }
    }
}

fn sev_colour(s: Severity) -> Color32 {
    match s {
        Severity::High => theme::BAD,
        Severity::Medium => theme::WARN,
        Severity::Low => MUTED,
    }
}

// ------------------------------------------------------------ drawing ----

/// The shield outline, `r` is half the height.
fn shield_points(c: Pos2, r: f32) -> Vec<Pos2> {
    let w = r * 0.82;
    let mut pts = vec![
        c + vec2(-w, -r * 0.72),
        c + vec2(0.0, -r),
        c + vec2(w, -r * 0.72),
    ];
    // Right side curving into the bottom point, then the left side back up.
    let steps = 14;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let x = w * (1.0 - t * t);
        let y = -r * 0.72 + (r * 1.72) * t.powf(0.85);
        pts.push(c + vec2(x, y));
    }
    for i in (0..steps).rev() {
        let t = i as f32 / steps as f32;
        let x = w * (1.0 - t * t);
        let y = -r * 0.72 + (r * 1.72) * t.powf(0.85);
        pts.push(c + vec2(-x, y));
    }
    pts
}

fn arc(painter: &egui::Painter, c: Pos2, r: f32, from: f32, to: f32, stroke: Stroke) {
    let n = ((to - from).abs() * r / 4.0).clamp(8.0, 160.0) as usize;
    let pts: Vec<Pos2> = (0..=n)
        .map(|i| {
            let a = from + (to - from) * i as f32 / n as f32;
            c + vec2(a.cos(), a.sin()) * r
        })
        .collect();
    painter.add(egui::Shape::line(pts, stroke));
}

#[derive(Clone, Copy, PartialEq)]
enum Status {
    Protected,
    Attention,
    Scanning,
}

/// The big emblem at the centre of the dashboard.
fn paint_emblem(painter: &egui::Painter, c: Pos2, r: f32, status: Status, t: f32) {
    let col = match status {
        Status::Protected => GREEN,
        Status::Attention => theme::BAD,
        Status::Scanning => BLUE,
    };
    // Soft halo.
    for i in 0..8 {
        let k = i as f32 / 8.0;
        painter.circle_filled(
            c,
            r * (1.25 - k * 0.2),
            col.linear_multiply(0.004 + k * 0.006),
        );
    }
    // Ring track and coloured arc.
    painter.circle_stroke(c, r, Stroke::new(r * 0.07, CARD_HI));
    let tau = std::f32::consts::TAU;
    match status {
        Status::Scanning => {
            let a = t * 3.2;
            arc(
                painter,
                c,
                r,
                a,
                a + tau * 0.32,
                Stroke::new(r * 0.07, BLUE),
            );
            arc(
                painter,
                c,
                r,
                a + tau * 0.5,
                a + tau * 0.62,
                Stroke::new(r * 0.07, BLUE.linear_multiply(0.5)),
            );
        }
        _ => {
            let start = -tau / 4.0;
            arc(
                painter,
                c,
                r,
                start,
                start + tau * 0.999,
                Stroke::new(r * 0.07, col),
            );
        }
    }
    // White disc with the shield.
    painter.circle_filled(c, r * 0.8, Color32::from_rgb(240, 244, 249));
    painter.circle_stroke(
        c,
        r * 0.8,
        Stroke::new(1.0_f32, Color32::from_rgb(210, 218, 228)),
    );
    let s = r * 0.5;
    let pts = shield_points(c + vec2(0.0, -r * 0.02), s);
    painter.add(egui::Shape::convex_polygon(
        pts.clone(),
        Color32::from_rgb(30, 42, 60),
        Stroke::NONE,
    ));
    let inner: Vec<Pos2> = shield_points(c + vec2(0.0, -r * 0.02), s * 0.84);
    painter.add(egui::Shape::convex_polygon(
        inner,
        Color32::from_rgb(38, 58, 84),
        Stroke::new(s * 0.04, col),
    ));
    let k = c + vec2(0.0, -r * 0.02);
    let w = Stroke::new(s * 0.13, Color32::WHITE);
    match status {
        Status::Protected => {
            painter.line_segment([k + vec2(-s * 0.32, 0.0), k + vec2(-s * 0.07, s * 0.26)], w);
            painter.line_segment(
                [k + vec2(-s * 0.07, s * 0.26), k + vec2(s * 0.36, -s * 0.24)],
                w,
            );
        }
        Status::Attention => {
            painter.line_segment([k + vec2(0.0, -s * 0.38), k + vec2(0.0, s * 0.12)], w);
            painter.circle_filled(k + vec2(0.0, s * 0.34), s * 0.08, Color32::WHITE);
        }
        Status::Scanning => {
            // Keyhole, like the design.
            painter.circle_filled(k + vec2(0.0, -s * 0.12), s * 0.16, Color32::WHITE);
            painter.add(egui::Shape::convex_polygon(
                vec![
                    k + vec2(-s * 0.08, -s * 0.05),
                    k + vec2(s * 0.08, -s * 0.05),
                    k + vec2(s * 0.13, s * 0.36),
                    k + vec2(-s * 0.13, s * 0.36),
                ],
                Color32::WHITE,
                Stroke::NONE,
            ));
        }
    }
}

fn paint_sym(p: &egui::Painter, c: Pos2, s: f32, sym: Sym, col: Color32) {
    let st = Stroke::new((s * 0.11).max(1.2), col);
    match sym {
        Sym::Grid => {
            for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                p.rect_stroke(
                    Rect::from_center_size(
                        c + vec2(dx * s * 0.26, dy * s * 0.26),
                        vec2(s * 0.36, s * 0.36),
                    ),
                    2.0,
                    st,
                );
            }
        }
        Sym::Search => {
            p.circle_stroke(c + vec2(-s * 0.08, -s * 0.08), s * 0.28, st);
            p.line_segment(
                [c + vec2(s * 0.13, s * 0.13), c + vec2(s * 0.42, s * 0.42)],
                st,
            );
        }
        Sym::Bug => {
            p.circle_stroke(c + vec2(0.0, s * 0.08), s * 0.26, st);
            p.circle_stroke(c + vec2(0.0, -s * 0.28), s * 0.12, st);
            for dy in [-0.05, 0.12, 0.3] {
                p.line_segment(
                    [
                        c + vec2(-s * 0.26, s * dy),
                        c + vec2(-s * 0.46, s * (dy - 0.05)),
                    ],
                    st,
                );
                p.line_segment(
                    [
                        c + vec2(s * 0.26, s * dy),
                        c + vec2(s * 0.46, s * (dy - 0.05)),
                    ],
                    st,
                );
            }
        }
        Sym::Box => {
            p.rect_stroke(
                Rect::from_center_size(c + vec2(0.0, s * 0.08), vec2(s * 0.76, s * 0.56)),
                2.0,
                st,
            );
            p.line_segment(
                [
                    c + vec2(-s * 0.44, -s * 0.28),
                    c + vec2(s * 0.44, -s * 0.28),
                ],
                st,
            );
            p.line_segment(
                [c + vec2(-s * 0.12, s * 0.02), c + vec2(s * 0.12, s * 0.02)],
                st,
            );
        }
        Sym::Globe => {
            p.circle_stroke(c, s * 0.38, st);
            p.line_segment([c + vec2(-s * 0.38, 0.0), c + vec2(s * 0.38, 0.0)], st);
            let pts: Vec<Pos2> = (0..=20)
                .map(|i| {
                    let a = std::f32::consts::PI * (i as f32 / 20.0 - 0.5);
                    c + vec2(s * 0.16 * a.cos(), s * 0.38 * a.sin())
                })
                .collect();
            p.add(egui::Shape::line(pts.clone(), st));
            p.add(egui::Shape::line(
                pts.iter().map(|q| pos2(2.0 * c.x - q.x, q.y)).collect(),
                st,
            ));
        }
        Sym::Gear => {
            p.circle_stroke(c, s * 0.2, st);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0;
                let d = vec2(a.cos(), a.sin());
                p.line_segment([c + d * s * 0.28, c + d * s * 0.42], st);
            }
        }
        Sym::Bolt => {
            let pts = vec![
                c + vec2(s * 0.08, -s * 0.44),
                c + vec2(-s * 0.2, s * 0.06),
                c + vec2(s * 0.02, s * 0.06),
                c + vec2(-s * 0.08, s * 0.44),
                c + vec2(s * 0.22, -s * 0.08),
                c + vec2(0.0, -s * 0.08),
                c + vec2(s * 0.08, -s * 0.44),
            ];
            p.add(egui::Shape::line(pts, st));
        }
        Sym::Data => {
            for dy in [-0.26, 0.0, 0.26] {
                p.rect_stroke(
                    Rect::from_center_size(c + vec2(0.0, s * dy), vec2(s * 0.7, s * 0.18)),
                    s * 0.09,
                    st,
                );
            }
        }
        Sym::Clock => {
            p.circle_stroke(c, s * 0.38, st);
            p.line_segment([c, c + vec2(0.0, -s * 0.24)], st);
            p.line_segment([c, c + vec2(s * 0.18, s * 0.08)], st);
        }
    }
}

/// A protection tile: small shield badge, title, status line.
fn tile(
    ui: &mut egui::Ui,
    w: f32,
    h: f32,
    sym: Sym,
    title: &str,
    value: &str,
    col: Color32,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
    let p = ui.painter();
    let fill = if resp.hovered() { CARD_HI } else { CARD };
    p.rect(
        rect,
        6.0,
        fill,
        Stroke::new(
            1.0_f32,
            if resp.hovered() {
                BLUE.linear_multiply(0.6)
            } else {
                LINE
            },
        ),
    );
    let bc = pos2(rect.min.x + 30.0, rect.center().y);
    p.add(egui::Shape::convex_polygon(
        shield_points(bc, 19.0),
        Color32::from_rgb(28, 36, 48),
        Stroke::new(1.6_f32, col),
    ));
    paint_sym(p, bc + vec2(0.0, -1.0), 20.0, sym, col);
    p.text(
        pos2(rect.min.x + 58.0, rect.center().y - 11.0),
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::proportional(13.5),
        TEXT,
    );
    p.text(
        pos2(rect.min.x + 58.0, rect.center().y + 10.0),
        egui::Align2::LEFT_CENTER,
        value,
        egui::FontId::proportional(12.0),
        col,
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn primary(ui: &mut egui::Ui, text: &str, enabled: bool) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(text).color(Color32::WHITE).strong())
            .fill(BLUE)
            .rounding(4.0)
            .min_size(vec2(0.0, 30.0)),
    )
}

fn secondary(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) -> egui::Response {
    ui.add(egui::Button::new(text).fill(CARD_HI).rounding(4.0))
}

fn chip(ui: &mut egui::Ui, s: Severity) {
    let col = sev_colour(s);
    egui::Frame::none()
        .fill(col.linear_multiply(0.18))
        .rounding(10.0)
        .inner_margin(egui::Margin::symmetric(8.0, 1.0))
        .show(ui, |ui| {
            ui.label(RichText::new(s.label()).small().strong().color(col));
        });
}

fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::none()
        .fill(CARD)
        .stroke(Stroke::new(1.0_f32, LINE))
        .rounding(6.0)
        .inner_margin(egui::Margin::same(10.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

fn section(
    ui: &mut egui::Ui,
    title: &str,
    count: String,
    open: bool,
    add: impl FnOnce(&mut egui::Ui),
) {
    egui::CollapsingHeader::new(
        RichText::new(format!("{title}  ·  {count}"))
            .strong()
            .color(TEXT),
    )
    .id_source(title)
    .default_open(open)
    .show(ui, add);
}

/// Actions chosen in the removal tab this frame.
#[derive(Default)]
struct RemovalActions {
    sweep: bool,
    disable: Vec<malware::StartupItem>,
    restore: Option<String>,
    end: Option<(u32, Option<PathBuf>, bool)>,
    open: Option<PathBuf>,
    run: Option<String>,
    url: Option<String>,
    hosts_fix: bool,
}

// ------------------------------------------------------------- window ----

impl Desktop {
    fn shield_status(&self) -> (Status, String) {
        if self.shield.scanning {
            return (
                Status::Scanning,
                format!("Scanning · {} files", self.shield.count),
            );
        }
        if self.shield.sweeping {
            return (Status::Scanning, "Checking for installed malware…".into());
        }
        let threats = self
            .shield
            .findings
            .iter()
            .filter(|f| f.severity >= Severity::Medium)
            .count()
            + self.shield.sweep.as_ref().map(|s| s.threats()).unwrap_or(0);
        if threats > 0 {
            (
                Status::Attention,
                format!(
                    "{threats} threat{} need attention",
                    if threats == 1 { "" } else { "s" }
                ),
            )
        } else if !self.cfg.shield_watch {
            (Status::Attention, "Real-time protection is off".into())
        } else {
            (Status::Protected, "You're protected".into())
        }
    }

    pub(super) fn start_scan(&mut self, targets: Vec<PathBuf>) {
        if self.shield.scanning || targets.is_empty() {
            return;
        }
        self.shield.scanning = true;
        self.shield.count = 0;
        self.shield.findings.clear();
        self.shield.summary = None;
        self.shield.notes.clear();
        if self.shield.tab != ShieldTab::Dashboard {
            self.shield.tab = ShieldTab::Scan;
        }
        self.shield.cancel = Arc::new(AtomicBool::new(false));
        let cancel = self.shield.cancel.clone();
        let opts = ScanOptions {
            use_clamav: self.cfg.use_clamav,
            ..Default::default()
        };
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let report = antivirus::scan(&targets, &opts, &cancel, &mut |n, p| {
                let _ = tx.send(Event::ScanProgress(n, p.display().to_string()));
                ctx.request_repaint();
            });
            let _ = tx.send(Event::ScanDone(report));
            ctx.request_repaint();
        });
    }

    pub(super) fn on_scan_done(&mut self, r: ScanReport) {
        self.shield.scanning = false;
        self.shield.summary = Some(format!(
            "{}{} files checked in {:.1}s · {} finding{}{}",
            if r.cancelled { "Stopped · " } else { "" },
            r.files_scanned,
            r.duration.as_secs_f32(),
            r.findings.len(),
            if r.findings.len() == 1 { "" } else { "s" },
            if r.clamav_used {
                " · Orbit engine + ClamAV"
            } else {
                " · Orbit engine"
            }
        ));
        self.shield.notes = r.notes;
        if !r.findings.is_empty() {
            self.notify(format!(
                "Orbit Shield found {} item(s) to review",
                r.findings.len()
            ));
            self.shield.tab = ShieldTab::Scan;
        }
        self.shield.findings = r.findings;
    }

    pub(super) fn start_sweep(&mut self) {
        if self.shield.sweeping {
            return;
        }
        self.shield.sweeping = true;
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Event::Sweep(Box::new(malware::sweep())));
            ctx.request_repaint();
        });
    }

    pub(super) fn on_sweep(&mut self, s: Sweep) {
        self.shield.sweeping = false;
        self.shield.deep = malware::deep_tools();
        let n = s.threats();
        self.notify(if n == 0 {
            "Malware sweep: nothing suspicious found".to_string()
        } else {
            format!("Malware sweep: {n} item(s) need attention")
        });
        self.shield.sweep = Some(s);
    }

    pub(super) fn on_temp_cleaned(&mut self, n: usize, bytes: u64, skipped: usize) {
        self.shield.cleaning = false;
        self.notify(format!(
            "Deleted {n} temp file(s), freed {}{}",
            human_bytes(bytes),
            if skipped > 0 {
                format!(" · {skipped} in use, left alone")
            } else {
                String::new()
            }
        ));
        if let Some(s) = &mut self.shield.sweep {
            s.temp = malware::temp_summary();
        }
    }

    fn refresh_network(&mut self) {
        self.shield.net_loading = true;
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Event::Network(
                orbit_core::network::connections().map_err(|e| e.to_string()),
            ));
            ctx.request_repaint();
        });
    }

    pub(super) fn on_network(&mut self, r: Result<Vec<orbit_core::network::Connection>, String>) {
        self.shield.net_loading = false;
        match r {
            Ok(v) => {
                self.shield.net = v;
                self.shield.net_err = None;
            }
            Err(e) => self.shield.net_err = Some(e),
        }
    }

    pub(super) fn on_realtime(&mut self, found: Vec<Finding>) {
        for f in found {
            if !self
                .shield
                .findings
                .iter()
                .any(|x| x.path == f.path && x.threat == f.threat)
            {
                self.notify(format!(
                    "Orbit Shield: {} in {}",
                    f.threat,
                    f.path.file_name().unwrap_or_default().to_string_lossy()
                ));
                self.shield.findings.insert(0, f);
            }
        }
        self.win.shield = true;
        self.shield.tab = ShieldTab::Scan;
    }

    fn quarantine_finding(&mut self, f: &Finding) {
        match antivirus::quarantine(&f.path, &f.threat) {
            Ok(_) => {
                self.shield.findings.retain(|x| x.path != f.path);
                self.shield.quarantine = antivirus::quarantine_list();
                self.notify(format!(
                    "Quarantined {}",
                    f.path.file_name().unwrap_or_default().to_string_lossy()
                ));
                self.refresh_files();
            }
            Err(e) => self.notify(format!("Quarantine failed: {e}")),
        }
    }

    pub(super) fn shield_window(&mut self, ctx: &egui::Context) {
        let mut open = self.win.shield;
        if !self.wm_visible(WinId::Shield) {
            self.shield_dialogs(ctx);
            return;
        }
        let frame = egui::Frame::window(&ctx.style())
            .fill(BG)
            .stroke(Stroke::new(1.0_f32, LINE))
            .inner_margin(egui::Margin::same(0.0));
        let mut ask: Option<String> = None;
        let mut scan: Option<Vec<PathBuf>> = None;
        let mut quarantine: Vec<Finding> = Vec::new();
        let mut ignore: Option<usize> = None;
        let mut restore: Option<String> = None;
        let mut update = false;
        let mut save_cfg = false;
        let mut ra = RemovalActions::default();
        // A fixed size that fits the screen; filling a resizable window would make it grow.
        let d = self.desk_rect;
        let size = vec2(
            (d.width() - 40.0).clamp(760.0, 960.0),
            (d.height() - 60.0).clamp(480.0, 620.0),
        );

        self.wm_window(
            WinId::Shield,
            RichText::new("Orbit Shield").color(TEXT),
            [300.0, 50.0, 940.0, 610.0],
        )
        .frame(frame)
        .resizable(false)
        .fixed_size(size)
        .open(&mut open)
        .show(ctx, |ui| {
            // Dark widgets to match the panels.
            let v = ui.visuals_mut();
            v.widgets.inactive.weak_bg_fill = CARD_HI;
            v.widgets.inactive.bg_fill = CARD_HI;
            v.widgets.hovered.weak_bg_fill = Color32::from_rgb(54, 67, 86);
            v.extreme_bg_color = Color32::from_rgb(20, 25, 32);
            v.override_text_color = Some(TEXT);
            let avail = ui.available_size();
            ui.set_min_size(avail);

            egui::SidePanel::left("shield_nav")
                .exact_width(178.0)
                .resizable(false)
                .frame(
                    egui::Frame::none()
                        .fill(SIDEBAR)
                        .inner_margin(egui::Margin {
                            left: 0.0,
                            right: 0.0,
                            top: 12.0,
                            bottom: 10.0,
                        }),
                )
                .show_inside(ui, |ui| self.shield_nav(ui));

            egui::CentralPanel::default()
                .frame(
                    egui::Frame::none()
                        .fill(BG)
                        .inner_margin(egui::Margin::symmetric(18.0, 14.0)),
                )
                .show_inside(ui, |ui| {
                    self.shield_header(ui);
                    ui.add_space(8.0);
                    match self.shield.tab {
                        ShieldTab::Dashboard => {
                            self.shield_dashboard(ui, &mut scan, &mut save_cfg, &mut ra)
                        }
                        ShieldTab::Scan => self.shield_scan_tab(
                            ui,
                            &mut scan,
                            &mut quarantine,
                            &mut ignore,
                            &mut ask,
                        ),
                        ShieldTab::Removal => self.shield_removal_tab(ui, &mut ask, &mut ra),
                        ShieldTab::Quarantine => self.shield_quarantine_tab(ui, &mut restore),
                        ShieldTab::Network => self.network_tab(ui, &mut ask),
                        ShieldTab::Protection => {
                            self.shield_protection_tab(ui, &mut save_cfg, &mut update, &mut ra)
                        }
                    }
                });
        });
        self.win.shield = open;

        if save_cfg {
            let _ = self.cfg.save();
        }
        if let Some(t) = scan {
            self.start_scan(t);
        }
        for f in quarantine {
            self.quarantine_finding(&f);
        }
        if let Some(i) = ignore {
            if i < self.shield.findings.len() {
                self.shield.findings.remove(i);
            }
        }
        if let Some(q) = ask {
            self.send_to_claude(q);
        }
        if let Some(id) = restore {
            match antivirus::restore(&id) {
                Ok(p) => self.notify(format!("Restored {}", p.display())),
                Err(e) => self.notify(format!("Restore failed: {e}")),
            }
            self.shield.quarantine = antivirus::quarantine_list();
            self.refresh_files();
        }
        if update {
            self.shield.updating = true;
            let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
            std::thread::spawn(move || {
                let _ = tx.send(Event::ShieldUpdate(
                    antivirus::update_clamav().map_err(|e| e.to_string()),
                ));
                ctx.request_repaint();
            });
        }
        self.apply_removal(ra);
        self.shield_dialogs(ctx);
    }

    fn apply_removal(&mut self, ra: RemovalActions) {
        if ra.sweep {
            self.start_sweep();
        }
        for item in ra.disable {
            match malware::disable_startup(&item) {
                Ok(()) => {
                    self.notify(format!(
                        "Disabled {} at sign-in (you can restore it)",
                        item.name
                    ));
                    if let Some(s) = &mut self.shield.sweep {
                        s.startup.retain(|i| i.id != item.id);
                    }
                }
                Err(e) => self.notify(format!("Couldn't disable {}: {e}", item.name)),
            }
            self.shield.disabled = malware::disabled_list();
        }
        if let Some(id) = ra.restore {
            match malware::restore_startup(&id) {
                Ok(()) => self.notify("Startup item restored"),
                Err(e) => self.notify(format!("Restore failed: {e}")),
            }
            self.shield.disabled = malware::disabled_list();
        }
        if let Some((pid, exe, quarantine)) = ra.end {
            match malware::end_process(pid) {
                Ok(()) => {
                    let mut msg = format!("Ended program {pid}");
                    if let (true, Some(exe)) = (quarantine, exe) {
                        std::thread::sleep(Duration::from_millis(300));
                        match antivirus::quarantine(&exe, "Removed running program") {
                            Ok(_) => {
                                msg.push_str(" and quarantined its file");
                                self.shield.quarantine = antivirus::quarantine_list();
                            }
                            Err(e) => msg.push_str(&format!("; quarantine failed: {e}")),
                        }
                    }
                    self.notify(msg);
                    if let Some(s) = &mut self.shield.sweep {
                        s.processes.retain(|p| p.pid != pid);
                    }
                }
                Err(e) => self.notify(format!("Couldn't end it: {e}")),
            }
        }
        if let Some(p) = ra.open {
            let _ = exec::open(&p.to_string_lossy());
        }
        if let Some(u) = ra.url {
            let _ = exec::open(&u);
        }
        if let Some(cmd) = ra.run {
            self.pending_run = Some(cmd);
        }
        if ra.hosts_fix {
            let issues = self
                .shield
                .sweep
                .as_ref()
                .map(|s| s.hosts.clone())
                .unwrap_or_default();
            match malware::hosts_repair_command(&issues) {
                Ok(cmd) => self.pending_run = Some(cmd),
                Err(e) => self.notify(format!("Hosts repair: {e}")),
            }
        }
    }

    fn shield_nav(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            let (r, _) = ui.allocate_exact_size(vec2(26.0, 30.0), Sense::hover());
            ui.painter().add(egui::Shape::convex_polygon(
                shield_points(r.center(), 13.0),
                BLUE,
                Stroke::NONE,
            ));
            paint_sym(ui.painter(), r.center(), 12.0, Sym::Bolt, Color32::WHITE);
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("ORBIT SHIELD")
                        .strong()
                        .size(13.0)
                        .color(TEXT),
                );
                ui.label(RichText::new("Antivirus & removal").size(10.5).color(MUTED));
            });
        });
        ui.add_space(14.0);
        let threats = self.shield.sweep.as_ref().map(|s| s.threats()).unwrap_or(0);
        let items: [(ShieldTab, Sym, String); 6] = [
            (ShieldTab::Dashboard, Sym::Grid, "Dashboard".into()),
            (
                ShieldTab::Scan,
                Sym::Search,
                if self.shield.findings.is_empty() {
                    "Virus scan".into()
                } else {
                    format!("Virus scan ({})", self.shield.findings.len())
                },
            ),
            (
                ShieldTab::Removal,
                Sym::Bug,
                if threats == 0 {
                    "Malware removal".into()
                } else {
                    format!("Malware removal ({threats})")
                },
            ),
            (
                ShieldTab::Quarantine,
                Sym::Box,
                format!("Quarantine ({})", self.shield.quarantine.len()),
            ),
            (ShieldTab::Network, Sym::Globe, "Network".into()),
            (ShieldTab::Protection, Sym::Gear, "Protection".into()),
        ];
        for (tab, sym, label) in items {
            let (rect, resp) =
                ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
            let active = self.shield.tab == tab;
            let p = ui.painter();
            if active {
                p.rect_filled(rect.shrink2(vec2(8.0, 2.0)), 4.0, BLUE);
            } else if resp.hovered() {
                p.rect_filled(rect.shrink2(vec2(8.0, 2.0)), 4.0, CARD);
            }
            let col = if active { Color32::WHITE } else { MUTED };
            paint_sym(p, pos2(rect.min.x + 30.0, rect.center().y), 16.0, sym, col);
            p.text(
                pos2(rect.min.x + 48.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                label,
                egui::FontId::proportional(13.0),
                if active { Color32::WHITE } else { TEXT },
            );
            if resp
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                self.shield.tab = tab;
            }
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(format!("{} signature(s)", self.shield.signatures))
                            .size(10.5)
                            .color(MUTED),
                    );
                    ui.label(
                        RichText::new(if self.shield.clamav.is_some() {
                            "ClamAV on"
                        } else {
                            "ClamAV not installed"
                        })
                        .size(10.5)
                        .color(MUTED),
                    );
                    ui.label(
                        RichText::new(if self.shield.yara {
                            "YARA rules on"
                        } else {
                            "YARA not installed"
                        })
                        .size(10.5)
                        .color(MUTED),
                    );
                });
            });
        });
    }

    fn shield_header(&mut self, ui: &mut egui::Ui) {
        let title = match self.shield.tab {
            ShieldTab::Dashboard => "Protection overview",
            ShieldTab::Scan => "Virus scan",
            ShieldTab::Removal => "Malware removal",
            ShieldTab::Quarantine => "Quarantine",
            ShieldTab::Network => "Network",
            ShieldTab::Protection => "Protection settings",
        };
        let (status, text) = self.shield_status();
        ui.horizontal(|ui| {
            ui.label(RichText::new(title).size(20.0).strong().color(TEXT));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let col = match status {
                    Status::Protected => GREEN,
                    Status::Attention => theme::BAD,
                    Status::Scanning => BLUE,
                };
                egui::Frame::none()
                    .fill(col.linear_multiply(0.16))
                    .stroke(Stroke::new(1.0_f32, col.linear_multiply(0.6)))
                    .rounding(12.0)
                    .inner_margin(egui::Margin::symmetric(10.0, 3.0))
                    .show(ui, |ui| {
                        ui.label(RichText::new(format!("●  {text}")).size(12.0).color(col));
                    });
            });
        });
        let r = ui.available_rect_before_wrap();
        ui.painter()
            .hline(r.x_range(), r.min.y + 4.0, Stroke::new(1.0_f32, LINE));
        ui.add_space(8.0);
    }

    fn shield_dashboard(
        &mut self,
        ui: &mut egui::Ui,
        scan: &mut Option<Vec<PathBuf>>,
        save_cfg: &mut bool,
        ra: &mut RemovalActions,
    ) {
        let (status, text) = self.shield_status();
        let t = ui.input(|i| i.time) as f32;
        if status == Status::Scanning {
            ui.ctx().request_repaint_after(Duration::from_millis(33));
        }
        let w = ui.available_width();
        let tile_w = ((w - 40.0) * 0.3).clamp(170.0, 260.0);
        let centre_w = w - tile_w * 2.0 - 24.0;
        let th = ((ui.available_height() - 150.0 - 24.0) / 3.0).clamp(64.0, 104.0);
        let h = 3.0 * th + 2.0 * 12.0;
        let sweep = self.shield.sweep.as_ref();
        let removal_val = match (self.shield.sweeping, sweep) {
            (true, _) => ("Checking…".to_string(), BLUE),
            (_, Some(s)) if s.threats() > 0 => {
                (format!("{} need attention", s.threats()), theme::BAD)
            }
            (_, Some(_)) => ("Nothing installed found".to_string(), GREEN),
            _ => ("Not run yet".to_string(), theme::WARN),
        };
        let established = self
            .shield
            .net
            .iter()
            .filter(|c| c.state.to_uppercase().starts_with("ESTAB"))
            .count();
        let mut go: Option<ShieldTab> = None;

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 12.0;
                if tile(
                    ui,
                    tile_w,
                    th,
                    Sym::Bolt,
                    "Real-time protection",
                    if self.cfg.shield_watch {
                        "On · watching new files"
                    } else {
                        "Off · click to turn on"
                    },
                    if self.cfg.shield_watch {
                        GREEN
                    } else {
                        theme::BAD
                    },
                )
                .on_hover_text("Checks new files in Downloads and Desktop as they arrive")
                .clicked()
                {
                    self.cfg.shield_watch = !self.cfg.shield_watch;
                    *save_cfg = true;
                }
                let sig = if self.shield.clamav.is_some() {
                    format!("{} + ClamAV", self.shield.signatures)
                } else {
                    format!("{} · add ClamAV", self.shield.signatures)
                };
                if tile(
                    ui,
                    tile_w,
                    th,
                    Sym::Data,
                    "Virus signatures",
                    &sig,
                    if self.shield.clamav.is_some() {
                        GREEN
                    } else {
                        theme::WARN
                    },
                )
                .clicked()
                {
                    go = Some(ShieldTab::Protection);
                }
                if tile(
                    ui,
                    tile_w,
                    th,
                    Sym::Bug,
                    "Malware removal",
                    &removal_val.0,
                    removal_val.1,
                )
                .clicked()
                {
                    go = Some(ShieldTab::Removal);
                    if sweep.is_none() {
                        ra.sweep = true;
                    }
                }
            });
            ui.add_space(12.0);
            let (rect, _) = ui.allocate_exact_size(vec2(centre_w, h), Sense::hover());
            let r = (rect.height() * 0.38).min(centre_w * 0.3);
            let c = rect.center() - vec2(0.0, 18.0);
            paint_emblem(ui.painter(), c, r, status, t);
            let col = match status {
                Status::Protected => GREEN,
                Status::Attention => theme::BAD,
                Status::Scanning => BLUE,
            };
            ui.painter().text(
                pos2(c.x, rect.max.y - 22.0),
                egui::Align2::CENTER_CENTER,
                &text,
                egui::FontId::proportional(17.0),
                col,
            );
            ui.add_space(12.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 12.0;
                let q = self.shield.quarantine.len();
                if tile(
                    ui,
                    tile_w,
                    th,
                    Sym::Box,
                    "Quarantine",
                    &if q == 0 {
                        "Empty".to_string()
                    } else {
                        format!("{q} held safely")
                    },
                    if q == 0 { GREEN } else { theme::WARN },
                )
                .clicked()
                {
                    go = Some(ShieldTab::Quarantine);
                }
                let net = if self.shield.net.is_empty() {
                    "View connections".to_string()
                } else {
                    format!("{established} active connections")
                };
                if tile(ui, tile_w, th, Sym::Globe, "Network guard", &net, BLUE).clicked() {
                    go = Some(ShieldTab::Network);
                }
                let sched = if self.cfg.scheduled_scan {
                    format!("Daily at {:02}:00", self.cfg.scan_hour)
                } else {
                    "Off".to_string()
                };
                if tile(
                    ui,
                    tile_w,
                    th,
                    Sym::Clock,
                    "Scheduled scan",
                    &sched,
                    if self.cfg.scheduled_scan {
                        GREEN
                    } else {
                        theme::WARN
                    },
                )
                .clicked()
                {
                    go = Some(ShieldTab::Protection);
                }
            });
        });
        if let Some(g) = go {
            self.shield.tab = g;
        }

        ui.add_space(14.0);
        card(ui, |ui| {
            if self.shield.scanning {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(
                        RichText::new(format!("{} files checked", self.shield.count)).color(TEXT),
                    );
                });
                ui.add(
                    egui::Label::new(RichText::new(&self.shield.current).small().color(MUTED))
                        .truncate(true),
                );
            } else {
                ui.label(RichText::new("Last scan").small().color(MUTED));
                ui.label(
                    RichText::new(self.shield.summary.clone().unwrap_or_else(|| {
                        "No scan yet this session. Run a quick scan to check your files.".into()
                    }))
                    .color(TEXT),
                );
            }
        });
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let idle = !self.shield.scanning;
            if primary(ui, "  Quick scan  ", idle).on_hover_text("Downloads, Desktop, Documents and temp").clicked() {
                *scan = Some(antivirus::quick_scan_paths());
            }
            if secondary(ui, "Full scan").on_hover_text("Your whole home folder").clicked() && idle {
                *scan = icons::dirs_home().map(|h| vec![h]);
            }
            if self.shield.scanning && secondary(ui, "Stop").clicked() {
                self.shield.cancel.store(true, Ordering::Relaxed);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if primary(ui, "  Malware sweep  ", !self.shield.sweeping)
                    .on_hover_text("Checks startup items, running programs, browser extensions and the hosts file")
                    .clicked()
                {
                    ra.sweep = true;
                    self.shield.tab = ShieldTab::Removal;
                }
            });
        });
    }

    fn shield_scan_tab(
        &mut self,
        ui: &mut egui::Ui,
        scan: &mut Option<Vec<PathBuf>>,
        quarantine: &mut Vec<Finding>,
        ignore: &mut Option<usize>,
        ask: &mut Option<String>,
    ) {
        ui.horizontal(|ui| {
            let idle = !self.shield.scanning;
            if primary(ui, "Quick scan", idle)
                .on_hover_text("Downloads, Desktop, Documents and temp")
                .clicked()
            {
                *scan = Some(antivirus::quick_scan_paths());
            }
            if primary(ui, "Full scan", idle)
                .on_hover_text("Your whole home folder")
                .clicked()
            {
                *scan = icons::dirs_home().map(|h| vec![h]);
            }
            ui.add(
                egui::TextEdit::singleline(&mut self.shield.custom)
                    .hint_text("folder or file to scan")
                    .desired_width(220.0),
            );
            if ui
                .add_enabled(
                    idle && !self.shield.custom.trim().is_empty(),
                    egui::Button::new("Scan").fill(CARD_HI),
                )
                .clicked()
            {
                *scan = Some(vec![orbit_core::expand_tilde(self.shield.custom.trim())]);
            }
            if self.shield.scanning && secondary(ui, "Stop").clicked() {
                self.shield.cancel.store(true, Ordering::Relaxed);
            }
        });
        ui.add_space(6.0);
        if self.shield.scanning {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!("{} files checked", self.shield.count));
            });
            ui.add(
                egui::Label::new(RichText::new(&self.shield.current).small().color(MUTED))
                    .truncate(true),
            );
            if self.cfg.use_clamav && self.shield.clamav.is_some() {
                ui.label(RichText::new("ClamAV runs after the Orbit engine; loading its database can take a little while.").small().color(MUTED));
            }
        }
        if let Some(s) = &self.shield.summary {
            ui.label(RichText::new(s).strong());
        }
        for n in &self.shield.notes {
            ui.label(RichText::new(n).small().color(theme::WARN));
        }
        let risky: Vec<Finding> = self
            .shield
            .findings
            .iter()
            .filter(|f| f.severity >= Severity::Medium)
            .cloned()
            .collect();
        if !risky.is_empty()
            && primary(ui, &format!("Remove all threats ({})", risky.len()), true)
                .on_hover_text("Quarantine every medium and high-risk file")
                .clicked()
        {
            quarantine.extend(risky);
        }
        ui.add_space(4.0);
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                for (i, f) in self.shield.findings.iter().enumerate() {
                    card(ui, |ui| {
                        ui.horizontal(|ui| {
                            chip(ui, f.severity);
                            ui.label(RichText::new(&f.threat).strong());
                            ui.label(RichText::new(format!("· {}", f.engine)).color(MUTED));
                        });
                        ui.add(
                            egui::Label::new(
                                RichText::new(f.path.display().to_string())
                                    .monospace()
                                    .small(),
                            )
                            .truncate(true),
                        )
                        .on_hover_text(f.path.display().to_string());
                        ui.label(RichText::new(&f.detail).color(MUTED));
                        ui.horizontal(|ui| {
                            if primary(ui, "Quarantine", true).clicked() {
                                quarantine.push(f.clone());
                            }
                            if secondary(ui, RichText::new("Ask Claude").color(theme::CLAUDE))
                                .clicked()
                            {
                                *ask = Some(f.describe());
                            }
                            if secondary(ui, "Ignore").clicked() {
                                *ignore = Some(i);
                            }
                        });
                    });
                    ui.add_space(6.0);
                }
                if self.shield.findings.is_empty() && !self.shield.scanning {
                    ui.label(
                        RichText::new(if self.shield.summary.is_some() {
                            "No threats found."
                        } else {
                            "Choose a scan to start."
                        })
                        .color(if self.shield.summary.is_some() {
                            GREEN
                        } else {
                            MUTED
                        }),
                    );
                }
            });
    }

    fn shield_removal_tab(
        &mut self,
        ui: &mut egui::Ui,
        ask: &mut Option<String>,
        ra: &mut RemovalActions,
    ) {
        ui.horizontal(|ui| {
            if primary(
                ui,
                if self.shield.sweep.is_some() {
                    "Sweep again"
                } else {
                    "Run malware sweep"
                },
                !self.shield.sweeping,
            )
            .clicked()
            {
                ra.sweep = true;
            }
            if self.shield.sweeping {
                ui.spinner();
                ui.label(
                    RichText::new(
                        "Checking startup items, running programs, extensions and the hosts file…",
                    )
                    .color(MUTED),
                );
            } else if let Some(s) = &self.shield.sweep {
                let n = s.threats();
                ui.label(
                    RichText::new(if n == 0 {
                        "Nothing suspicious installed.".to_string()
                    } else {
                        format!("{n} item(s) need attention")
                    })
                    .color(if n == 0 { GREEN } else { theme::BAD })
                    .strong(),
                );
                if n > 0 && primary(ui, "Remove all threats", true).clicked() {
                    self.shield.confirm_remove_all = true;
                }
            }
        });
        ui.add_space(6.0);
        egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
            let Some(s) = self.shield.sweep.clone() else {
                card(ui, |ui| {
                    ui.label(RichText::new("Malware that is already installed hides where file scans don't look: programs that start when you sign in, running processes, browser extensions and the hosts file. A sweep checks all of them in a few seconds and nothing is changed until you choose.").color(MUTED));
                });
                ui.add_space(8.0);
                self.deep_tools_section(ui, ra);
                return;
            };

            // Startup items.
            let flagged = s.startup.iter().filter(|i| !i.flags.is_empty()).count();
            section(ui, "Startup items", format!("{} total, {flagged} flagged", s.startup.len()), true, |ui| {
                ui.checkbox(&mut self.shield.show_all_startup, "Show items with no warnings too");
                for it in s.startup.iter().filter(|i| self.shield.show_all_startup || !i.flags.is_empty()) {
                    card(ui, |ui| {
                        ui.horizontal(|ui| {
                            chip(ui, it.severity);
                            ui.label(RichText::new(&it.name).strong());
                            ui.label(RichText::new(&it.location).small().color(MUTED));
                        });
                        ui.add(egui::Label::new(RichText::new(&it.command).monospace().small()).truncate(true)).on_hover_text(&it.command);
                        for f in &it.flags {
                            ui.label(RichText::new(format!("• {f}")).small().color(sev_colour(it.severity)));
                        }
                        ui.horizontal(|ui| {
                            if it.can_disable() {
                                if primary(ui, "Disable at sign-in", true).on_hover_text("Reversible: Orbit keeps a copy and you can restore it below").clicked() {
                                    ra.disable.push(it.clone());
                                }
                            } else {
                                ui.label(RichText::new("Needs an administrator to change").small().color(MUTED));
                            }
                            if secondary(ui, RichText::new("Ask Claude").color(theme::CLAUDE)).clicked() {
                                *ask = Some(it.describe());
                            }
                        });
                    });
                    ui.add_space(4.0);
                }
                if flagged == 0 && !self.shield.show_all_startup {
                    ui.label(RichText::new("No startup item looks suspicious.").color(GREEN));
                }
            });

            // Running programs.
            section(ui, "Running programs", format!("{} flagged", s.processes.len()), !s.processes.is_empty(), |ui| {
                if s.processes.is_empty() {
                    ui.label(RichText::new("No running program looks suspicious.").color(GREEN));
                }
                for p in &s.processes {
                    card(ui, |ui| {
                        ui.horizontal(|ui| {
                            chip(ui, p.severity);
                            ui.label(RichText::new(&p.name).strong());
                            ui.label(RichText::new(format!("pid {}", p.pid)).small().color(MUTED));
                        });
                        if let Some(e) = &p.exe {
                            ui.add(egui::Label::new(RichText::new(e.display().to_string()).monospace().small()).truncate(true));
                        }
                        for f in &p.flags {
                            ui.label(RichText::new(format!("• {f}")).small().color(sev_colour(p.severity)));
                        }
                        ui.horizontal(|ui| {
                            if primary(ui, "End and quarantine", p.exe.as_ref().map(|e| e.is_file()).unwrap_or(false)).clicked() {
                                ra.end = Some((p.pid, p.exe.clone(), true));
                            }
                            if secondary(ui, "End program").clicked() {
                                ra.end = Some((p.pid, None, false));
                            }
                            if secondary(ui, RichText::new("Ask Claude").color(theme::CLAUDE)).clicked() {
                                *ask = Some(p.describe());
                            }
                        });
                    });
                    ui.add_space(4.0);
                }
            });

            // Browser extensions.
            let risky = s.extensions.iter().filter(|e| e.severity >= Severity::Medium).count();
            section(ui, "Browser extensions", format!("{} installed, {risky} risky", s.extensions.len()), risky > 0, |ui| {
                ui.label(RichText::new("Remove an extension from its browser's Extensions page so it doesn't come back with sync.").small().color(MUTED));
                if s.extensions.is_empty() {
                    ui.label(RichText::new("No browser extensions found.").color(MUTED));
                }
                for e in &s.extensions {
                    card(ui, |ui| {
                        ui.horizontal(|ui| {
                            chip(ui, e.severity);
                            ui.label(RichText::new(&e.name).strong());
                            ui.label(RichText::new(format!("{} · v{}", e.browser, e.version)).small().color(MUTED));
                        });
                        for f in &e.flags {
                            ui.label(RichText::new(format!("• {f}")).small().color(sev_colour(e.severity)));
                        }
                        ui.horizontal(|ui| {
                            if secondary(ui, RichText::new("Ask Claude").color(theme::CLAUDE)).clicked() {
                                *ask = Some(e.describe());
                            }
                            if secondary(ui, "Show folder").clicked() {
                                ra.open = Some(e.path.clone());
                            }
                        });
                    });
                    ui.add_space(4.0);
                }
            });

            // Hosts file.
            section(ui, "Hosts file", format!("{} issue(s)", s.hosts.len()), !s.hosts.is_empty(), |ui| {
                if let Some(e) = &s.hosts_error {
                    ui.label(RichText::new(e).color(theme::WARN));
                } else if s.hosts.is_empty() {
                    ui.label(RichText::new("The hosts file is clean.").color(GREEN));
                }
                for h in &s.hosts {
                    card(ui, |ui| {
                        ui.horizontal(|ui| {
                            chip(ui, h.severity);
                            ui.label(RichText::new(format!("Line {}", h.line)).strong());
                        });
                        ui.label(RichText::new(&h.text).monospace().small());
                        ui.label(RichText::new(&h.reason).small().color(sev_colour(h.severity)));
                    });
                    ui.add_space(4.0);
                }
                if !s.hosts.is_empty() {
                    ui.horizontal(|ui| {
                        if primary(ui, "Repair hosts file", true).on_hover_text("Comments out these lines; asks for your password. A backup is kept.").clicked() {
                            ra.hosts_fix = true;
                        }
                        if secondary(ui, RichText::new("Ask Claude").color(theme::CLAUDE)).clicked() {
                            *ask = Some(format!(
                                "Orbit Shield found these entries in my hosts file:\n{}\nWhat could have added them, and is it safe to remove them?",
                                s.hosts.iter().map(|h| format!("{} ({})", h.text, h.reason)).collect::<Vec<_>>().join("\n")
                            ));
                        }
                    });
                }
            });

            // Temp files.
            section(ui, "Temporary files", format!("{} old file(s), {}", s.temp.0, human_bytes(s.temp.1)), false, |ui| {
                ui.label(RichText::new("Malware droppers often hide in temp folders. This deletes files there that are more than a day old; files in use are skipped.").small().color(MUTED));
                ui.horizontal(|ui| {
                    if primary(ui, if self.shield.cleaning { "Cleaning…" } else { "Clean temp files" }, s.temp.0 > 0 && !self.shield.cleaning).clicked() {
                        self.shield.confirm_temp = true;
                    }
                });
            });

            self.deep_tools_section(ui, ra);

            // Disabled startup items.
            if !self.shield.disabled.is_empty() {
                section(ui, "Disabled by Orbit", format!("{}", self.shield.disabled.len()), false, |ui| {
                    for d in &self.shield.disabled {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&d.item.name).strong());
                            ui.label(RichText::new(&d.item.location).small().color(MUTED));
                            if secondary(ui, "Restore").clicked() {
                                ra.restore = Some(d.id.clone());
                            }
                        });
                    }
                });
            }
        });
    }

    fn deep_tools_section(&mut self, ui: &mut egui::Ui, ra: &mut RemovalActions) {
        let n = self.shield.deep.len();
        section(
            ui,
            "Deep-clean tools",
            format!("{n} available"),
            self.shield.sweep.is_none(),
            |ui| {
                ui.label(RichText::new("Free, official second-opinion tools for stubborn malware and rootkits. Orbit shows the command before it runs.").small().color(MUTED));
                for t in &self.shield.deep {
                    card(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(t.name).strong());
                            ui.label(
                                RichText::new(if t.installed {
                                    "installed"
                                } else {
                                    "not installed"
                                })
                                .small()
                                .color(if t.installed {
                                    GREEN
                                } else {
                                    MUTED
                                }),
                            );
                        });
                        ui.label(RichText::new(t.detail).small().color(MUTED));
                        ui.horizontal(|ui| {
                            if t.installed {
                                if let Some(r) = &t.run {
                                    if primary(ui, "Run", true).clicked() {
                                        ra.run = Some(r.clone());
                                    }
                                }
                            } else if let Some(i) = &t.install {
                                if primary(
                                    ui,
                                    if i.starts_with("https://") {
                                        "Download"
                                    } else {
                                        "Install"
                                    },
                                    true,
                                )
                                .clicked()
                                {
                                    if i.starts_with("https://") {
                                        ra.url = Some(i.clone());
                                    } else {
                                        ra.run = Some(i.clone());
                                    }
                                }
                            }
                        });
                    });
                    ui.add_space(4.0);
                }
            },
        );
    }

    fn shield_quarantine_tab(&mut self, ui: &mut egui::Ui, restore: &mut Option<String>) {
        ui.label(RichText::new("Quarantined files are encoded so they can't run. Restore puts a file back exactly where it was.").color(MUTED));
        ui.add_space(6.0);
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                if self.shield.quarantine.is_empty() {
                    ui.label(RichText::new("Quarantine is empty.").color(GREEN));
                }
                for q in &self.shield.quarantine {
                    card(ui, |ui| {
                        ui.label(RichText::new(&q.threat).strong());
                        ui.add(
                            egui::Label::new(
                                RichText::new(q.original.display().to_string())
                                    .monospace()
                                    .small(),
                            )
                            .truncate(true),
                        );
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(human_bytes(q.size)).color(MUTED));
                            if secondary(ui, "Restore").clicked() {
                                *restore = Some(q.id.clone());
                            }
                            if secondary(ui, RichText::new("Delete forever").color(theme::BAD))
                                .clicked()
                            {
                                self.shield.confirm_delete = Some(q.clone());
                            }
                        });
                    });
                    ui.add_space(4.0);
                }
            });
    }

    fn shield_protection_tab(
        &mut self,
        ui: &mut egui::Ui,
        save_cfg: &mut bool,
        update: &mut bool,
        ra: &mut RemovalActions,
    ) {
        card(ui, |ui| {
            ui.label(RichText::new("Real-time").strong());
            if ui
                .checkbox(
                    &mut self.cfg.shield_watch,
                    "Check new files in Downloads and Desktop as they arrive",
                )
                .changed()
            {
                *save_cfg = true;
            }
        });
        ui.add_space(6.0);
        card(ui, |ui| {
            ui.label(RichText::new("Scheduled scan").strong());
            ui.horizontal(|ui| {
                if ui
                    .checkbox(&mut self.cfg.scheduled_scan, "Quick scan every day at")
                    .changed()
                {
                    *save_cfg = true;
                }
                if ui
                    .add(
                        egui::DragValue::new(&mut self.cfg.scan_hour)
                            .clamp_range(0..=23)
                            .suffix(":00"),
                    )
                    .changed()
                {
                    *save_cfg = true;
                }
            });
            ui.label(
                RichText::new("Runs in the background, even when Orbit is in the tray.")
                    .small()
                    .color(MUTED),
            );
        });
        ui.add_space(6.0);
        card(ui, |ui| {
            ui.label(RichText::new("Engines").strong());
            ui.label(format!(
                "Orbit engine: {} signature(s) + heuristics",
                self.shield.signatures
            ));
            ui.horizontal(|ui| {
                if ui
                    .checkbox(&mut self.cfg.use_clamav, "Also scan with ClamAV")
                    .changed()
                {
                    *save_cfg = true;
                }
                ui.label(
                    RichText::new(
                        self.shield
                            .clamav
                            .clone()
                            .unwrap_or_else(|| "not installed".into()),
                    )
                    .small()
                    .color(MUTED),
                );
            });
            let label = if self.shield.updating {
                "Updating…"
            } else {
                "Update ClamAV signatures"
            };
            if primary(
                ui,
                label,
                !self.shield.updating && self.shield.clamav.is_some(),
            )
            .clicked()
            {
                *update = true;
            }
            if self.shield.clamav.is_none() {
                ui.label(
                    RichText::new(format!(
                        "Install ClamAV for millions more signatures: {}",
                        antivirus::clamav_install_hint()
                    ))
                    .small()
                    .color(MUTED),
                );
            }
            ui.label(format!(
                "YARA: {}",
                if self.shield.yara {
                    "installed, rules loaded"
                } else {
                    "not installed"
                }
            ));
            if secondary(ui, "Open YARA rules folder").clicked() {
                ra.open = Some(antivirus::yara_rules_dir());
            }
        });
    }

    fn network_tab(&mut self, ui: &mut egui::Ui, ask: &mut Option<String>) {
        ui.horizontal(|ui| {
            if primary(ui, "Refresh", true).clicked()
                || (self.shield.net.is_empty()
                    && !self.shield.net_loading
                    && self.shield.net_err.is_none())
            {
                self.refresh_network();
            }
            if self.shield.net_loading {
                ui.spinner();
            }
            ui.add(
                egui::TextEdit::singleline(&mut self.shield.net_filter)
                    .hint_text("Filter by app or address")
                    .desired_width(200.0),
            );
            let established = self
                .shield
                .net
                .iter()
                .filter(|c| c.state.to_uppercase().starts_with("ESTAB"))
                .count();
            ui.label(RichText::new(format!("{established} active connections")).color(MUTED));
        });
        if let Some(e) = &self.shield.net_err {
            ui.colored_label(theme::WARN, e);
        }
        let q = self.shield.net_filter.to_lowercase();
        let rows: Vec<orbit_core::network::Connection> = self
            .shield
            .net
            .iter()
            .filter(|c| {
                q.is_empty() || c.process.to_lowercase().contains(&q) || c.remote.contains(&q)
            })
            .filter(|c| c.remote_ip().is_some() || c.state.to_uppercase().contains("LISTEN"))
            .cloned()
            .collect();
        let mut block: Option<String> = None;
        egui::ScrollArea::vertical().auto_shrink([false; 2]).max_height(ui.available_height() - 24.0).show(ui, |ui| {
            egui::Grid::new("net").num_columns(5).striped(true).show(ui, |ui| {
                for h in ["App", "Protocol", "Remote address", "State", ""] {
                    ui.label(RichText::new(h).small().color(MUTED));
                }
                ui.end_row();
                for c in &rows {
                    ui.label(if c.process.is_empty() { c.pid.map(|p| format!("pid {p}")).unwrap_or_default() } else { c.process.clone() });
                    ui.label(&c.proto);
                    ui.label(RichText::new(if c.remote.is_empty() { c.local.clone() } else { c.remote.clone() }).monospace().small());
                    ui.label(RichText::new(&c.state).small());
                    ui.horizontal(|ui| {
                        if let Some(ip) = c.remote_ip() {
                            if secondary(ui, "Block").on_hover_text("Block outgoing traffic to this address (asks for your password)").clicked() {
                                block = Some(ip.clone());
                            }
                            if secondary(ui, RichText::new("Ask Claude").color(theme::CLAUDE)).clicked() {
                                *ask = Some(format!(
                                    "Orbit Shield's network view shows the program '{}' (pid {:?}) connected to {} over {} ({}). \
                                     Is this normal? What is it likely to be, and should I block it?",
                                    c.process, c.pid, c.remote, c.proto, c.state
                                ));
                            }
                        }
                    });
                    ui.end_row();
                }
            });
        });
        ui.label(
            RichText::new("Block adds a firewall rule using your system's own password prompt.")
                .small()
                .color(MUTED),
        );
        if let Some(ip) = block {
            if let Some(cmd) = orbit_core::network::block_command(&ip) {
                self.pending_run = Some(cmd);
            }
        }
    }

    // ------------------------------------------------------------ dialogs --

    fn shield_dialogs(&mut self, ctx: &egui::Context) {
        self.confirm_delete_dialog(ctx);
        self.confirm_temp_dialog(ctx);
        self.confirm_remove_all_dialog(ctx);
    }

    fn confirm_temp_dialog(&mut self, ctx: &egui::Context) {
        if !self.shield.confirm_temp {
            return;
        }
        let (n, bytes) = self.shield.sweep.as_ref().map(|s| s.temp).unwrap_or((0, 0));
        let mut decision = None;
        egui::Window::new("Delete old temp files?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(format!("{n} file(s), {}, older than a day in your temp folders.", human_bytes(bytes)));
                ui.label(RichText::new("They are deleted permanently, not moved to the bin. Files in use are left alone.").color(theme::WARN));
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("Delete").color(theme::BAD)).clicked() {
                        decision = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        decision = Some(false);
                    }
                });
            });
        match decision {
            Some(true) => {
                self.shield.confirm_temp = false;
                self.shield.cleaning = true;
                let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
                std::thread::spawn(move || {
                    let (n, b, s) = malware::clean_temp();
                    let _ = tx.send(Event::TempCleaned(n, b, s));
                    ctx.request_repaint();
                });
            }
            Some(false) => self.shield.confirm_temp = false,
            None => {}
        }
    }

    fn confirm_remove_all_dialog(&mut self, ctx: &egui::Context) {
        if !self.shield.confirm_remove_all {
            return;
        }
        let Some(s) = self.shield.sweep.clone() else {
            self.shield.confirm_remove_all = false;
            return;
        };
        let startup: Vec<_> = s
            .startup
            .iter()
            .filter(|i| i.severity >= Severity::Medium && i.can_disable())
            .cloned()
            .collect();
        let procs: Vec<_> = s
            .processes
            .iter()
            .filter(|p| p.severity >= Severity::Medium)
            .cloned()
            .collect();
        let files: Vec<Finding> = self
            .shield
            .findings
            .iter()
            .filter(|f| f.severity >= Severity::Medium)
            .cloned()
            .collect();
        let mut decision = None;
        egui::Window::new("Remove all threats?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label("Orbit Shield will:");
                ui.label(format!("• disable {} risky startup item(s) (reversible)", startup.len()));
                ui.label(format!("• end {} suspicious program(s) and quarantine their files", procs.len()));
                ui.label(format!("• quarantine {} risky file(s) from the last scan", files.len()));
                if !s.hosts.is_empty() {
                    ui.label(RichText::new("The hosts file is repaired separately, because it needs your password.").small().color(MUTED));
                }
                ui.label(RichText::new("Save your work in any program listed first.").color(theme::WARN));
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("Remove").strong().color(theme::BAD)).clicked() {
                        decision = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        decision = Some(false);
                    }
                });
            });
        match decision {
            Some(true) => {
                self.shield.confirm_remove_all = false;
                for p in procs {
                    self.apply_removal(RemovalActions {
                        end: Some((p.pid, p.exe.clone(), true)),
                        ..Default::default()
                    });
                }
                self.apply_removal(RemovalActions {
                    disable: startup,
                    ..Default::default()
                });
                for f in files {
                    self.quarantine_finding(&f);
                }
            }
            Some(false) => self.shield.confirm_remove_all = false,
            None => {}
        }
    }

    fn confirm_delete_dialog(&mut self, ctx: &egui::Context) {
        let Some(q) = self.shield.confirm_delete.clone() else {
            return;
        };
        let mut decision = None;
        egui::Window::new("Delete permanently?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(format!("{} ({})", q.original.display(), q.threat));
                ui.label(RichText::new("This can't be undone.").color(theme::BAD));
                ui.horizontal(|ui| {
                    if ui
                        .button(RichText::new("Delete").color(theme::BAD))
                        .clicked()
                    {
                        decision = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        decision = Some(false);
                    }
                });
            });
        match decision {
            Some(true) => {
                self.shield.confirm_delete = None;
                match antivirus::delete_quarantined(&q.id) {
                    Ok(()) => self.notify("Deleted permanently"),
                    Err(e) => self.notify(format!("Delete failed: {e}")),
                }
                self.shield.quarantine = antivirus::quarantine_list();
            }
            Some(false) => self.shield.confirm_delete = None,
            None => {}
        }
    }
}
