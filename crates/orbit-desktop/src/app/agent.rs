//! Background agent: the work that keeps going while Orbit is hidden.
//! Orbit Shield's on-arrival watch, the daily scheduled scan, and commands
//! from later launches (show, hide, toggle, quit). It never touches the UI
//! directly: it sends events and wakes the UI only when there's news.

use super::tray::{notify_native, TrayCmd, WinCtl};
use super::*;
use orbit_core::antivirus::{self, ScanOptions};
use orbit_core::instance::{Command, Primary};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

pub(super) fn spawn(ctl: Arc<WinCtl>, primary: Option<Primary>) {
    std::thread::Builder::new()
        .name("orbit-agent".into())
        .spawn(move || run(ctl, primary))
        .ok();
}

fn run(ctl: Arc<WinCtl>, primary: Option<Primary>) {
    let mut cfg = Config::load();
    let mut cfg_read = Instant::now();
    let mut live: Option<orbit_core::watch::LiveWatcher> = None;
    let mut poller: Option<antivirus::Watcher> = None;
    let mut last_poll = Instant::now();
    let mut last_schedule = Instant::now() - Duration::from_secs(3600);
    let mut scan_day = cfg.last_scheduled_scan_day;

    loop {
        // Commands from a second launch.
        if let Some(p) = &primary {
            for c in p.poll() {
                match c {
                    Command::Show => ctl.show(),
                    Command::Hide => ctl.hide(),
                    Command::Toggle => ctl.toggle(),
                    Command::Search => {
                        ctl.show();
                        ctl.send(Event::Tray(TrayCmd::Search));
                    }
                    Command::Quit => {
                        ctl.show();
                        ctl.send(Event::Tray(TrayCmd::Quit));
                    }
                }
            }
        }

        // Settings can change in the UI; re-read them now and then.
        if cfg_read.elapsed() > Duration::from_secs(20) {
            cfg = Config::load();
            cfg_read = Instant::now();
        }

        // Orbit Shield: on-arrival watch (OS events, polling as a fallback).
        if cfg.shield_watch {
            if live.is_none() && poller.is_none() {
                match orbit_core::watch::LiveWatcher::new(&antivirus::watch_paths()) {
                    Ok(w) => live = Some(w),
                    Err(_) => poller = Some(antivirus::Watcher::new(antivirus::watch_paths())),
                }
            }
        } else {
            live = None;
            poller = None;
        }
        let arrived = if let Some(w) = live.as_mut() {
            w.ready()
        } else if let (Some(p), true) = (
            poller.as_mut(),
            last_poll.elapsed() > Duration::from_secs(4),
        ) {
            last_poll = Instant::now();
            p.poll()
        } else {
            Vec::new()
        };
        if !arrived.is_empty() {
            let opts = ScanOptions {
                use_clamav: false,
                ..Default::default()
            };
            let r = antivirus::scan(&arrived, &opts, &AtomicBool::new(false), &mut |_, _| {});
            if !r.findings.is_empty() {
                if ctl.is_hidden() {
                    let f = &r.findings[0];
                    let name = f
                        .path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    notify_native(
                        "Orbit Shield",
                        &format!("{} in {}. Open Orbit to review it.", f.threat, name),
                    );
                }
                ctl.send(Event::Realtime(r.findings));
            }
        }

        // Daily scheduled quick scan.
        if last_schedule.elapsed() > Duration::from_secs(60) {
            last_schedule = Instant::now();
            let hour = chrono::Timelike::hour(&chrono::Local::now());
            let mut probe = cfg.clone();
            probe.last_scheduled_scan_day = scan_day.max(cfg.last_scheduled_scan_day);
            if antivirus::scheduled_scan_due(&probe, hour) {
                scan_day = orbit_core::today();
                ctl.send(Event::ScheduledScan(scan_day));
                let opts = ScanOptions {
                    use_clamav: cfg.use_clamav,
                    ..Default::default()
                };
                let r = antivirus::scan(
                    &antivirus::quick_scan_paths(),
                    &opts,
                    &AtomicBool::new(false),
                    &mut |_, _| {},
                );
                if ctl.is_hidden() && !r.findings.is_empty() {
                    notify_native(
                        "Orbit Shield",
                        &format!("Daily scan found {} item(s) to review.", r.findings.len()),
                    );
                }
                ctl.send(Event::ScanDone(r));
            }
        }

        std::thread::sleep(Duration::from_millis(250));
    }
}
