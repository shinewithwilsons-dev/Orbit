//! System tray, global show/hide shortcut, and showing or hiding the window
//! from any thread.
//!
//! A hidden window gets no frames on Windows and macOS, so showing it again
//! can't wait for the UI loop: `WinCtl::show` talks to the OS directly
//! (ShowWindow on Windows, NSApp unhide on macOS, a viewport command on Linux).

use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// What a tray menu item or a second launch asks the UI to do.
#[derive(Debug, Clone, Copy)]
pub(super) enum TrayCmd {
    Search,
    Ask,
    Scan,
    Quit,
}

/// Shared handle for showing and hiding the window from any thread.
pub(super) struct WinCtl {
    pub ctx: egui::Context,
    pub hidden: AtomicBool,
    /// HWND (Windows) or NSView pointer (macOS); 0 until the first frame.
    pub handle: AtomicUsize,
    pub tray_ok: AtomicBool,
    tx: Mutex<Sender<Event>>,
}

impl WinCtl {
    pub fn new(ctx: egui::Context, tx: Sender<Event>) -> Arc<Self> {
        Arc::new(Self {
            ctx,
            hidden: AtomicBool::new(false),
            handle: AtomicUsize::new(0),
            tray_ok: AtomicBool::new(false),
            tx: Mutex::new(tx),
        })
    }

    pub fn is_hidden(&self) -> bool {
        self.hidden.load(Ordering::Relaxed)
    }

    /// Send an event to the UI and wake it.
    pub fn send(&self, ev: Event) {
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(ev);
        }
        self.ctx.request_repaint();
    }

    pub fn show(&self) {
        self.hidden.store(false, Ordering::Relaxed);
        #[cfg(windows)]
        platform::show(self.handle.load(Ordering::Relaxed));
        #[cfg(target_os = "macos")]
        platform::show();
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            self.ctx
                .send_viewport_cmd(egui::ViewportCommand::Visible(true));
            self.ctx
                .send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        }
        self.ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        self.ctx.request_repaint();
    }

    pub fn hide(&self) {
        // Never hide without a tray icon: nothing could bring Orbit back.
        if !self.tray_ok.load(Ordering::Relaxed) {
            self.ctx
                .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            self.ctx.request_repaint();
            return;
        }
        self.hidden.store(true, Ordering::Relaxed);
        #[cfg(windows)]
        platform::hide(self.handle.load(Ordering::Relaxed));
        #[cfg(target_os = "macos")]
        platform::hide();
        #[cfg(not(any(windows, target_os = "macos")))]
        self.ctx
            .send_viewport_cmd(egui::ViewportCommand::Visible(false));
        self.ctx.request_repaint();
    }

    pub fn toggle(&self) {
        if self.is_hidden() {
            self.show()
        } else {
            self.hide()
        }
    }
}

/// Raw window handle of the main window, as a number.
pub(super) fn window_handle(frame: &eframe::Frame) -> usize {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match frame.window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Win32(w)) => w.hwnd.get() as usize,
        Ok(RawWindowHandle::AppKit(a)) => a.ns_view.as_ptr() as usize,
        _ => 0,
    }
}

#[cfg(windows)]
pub(super) mod platform {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        IsIconic, SetForegroundWindow, ShowWindowAsync, SW_HIDE, SW_RESTORE, SW_SHOW,
    };

    fn hwnd(h: usize) -> windows_sys::Win32::Foundation::HWND {
        h as _
    }

    pub fn show(h: usize) {
        if h == 0 {
            return;
        }
        unsafe {
            if IsIconic(hwnd(h)) != 0 {
                ShowWindowAsync(hwnd(h), SW_RESTORE);
            } else {
                ShowWindowAsync(hwnd(h), SW_SHOW);
            }
            SetForegroundWindow(hwnd(h));
        }
    }

    pub fn hide(h: usize) {
        if h != 0 {
            unsafe {
                ShowWindowAsync(hwnd(h), SW_HIDE);
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub(super) mod platform {
    use objc2::runtime::{AnyClass, AnyObject, Bool, Sel};
    use objc2::{msg_send, sel};

    /// Run an NSApplication action (`hide:` / `unhide:`) on the main thread.
    fn app_action(action: Sel) {
        let Some(cls) = AnyClass::get(c"NSApplication") else {
            return;
        };
        unsafe {
            let app: *mut AnyObject = msg_send![cls, sharedApplication];
            if app.is_null() {
                return;
            }
            let none: *mut AnyObject = std::ptr::null_mut();
            let _: () = msg_send![app, performSelectorOnMainThread: action, withObject: none, waitUntilDone: Bool::NO];
        }
    }

    /// Like Cmd+H: the app keeps running, its windows leave the screen.
    pub fn hide() {
        app_action(sel!(hide:));
    }

    /// Brings the windows back and makes Orbit the active app.
    pub fn show() {
        app_action(sel!(unhide:));
    }
}

// ---------------------------------------------------------------- tray --

fn tray_icon_image() -> Option<tray_icon::Icon> {
    let img = image::load_from_memory(include_bytes!("../../../../packaging/icons/icon-64.png"))
        .ok()?
        .to_rgba8();
    let (w, h) = img.dimensions();
    tray_icon::Icon::from_rgba(img.into_raw(), w, h).ok()
}

fn build_tray() -> Result<tray_icon::TrayIcon, String> {
    use tray_icon::menu::{Menu, MenuItem, PredefinedMenuItem};
    let menu = Menu::new();
    menu.append_items(&[
        &MenuItem::with_id("open", "Open Orbit", true, None),
        &MenuItem::with_id("search", "Search\u{2026}", true, None),
        &MenuItem::with_id("ask", "Ask Claude", true, None),
        &MenuItem::with_id("scan", "Quick scan", true, None),
        &PredefinedMenuItem::separator(),
        &MenuItem::with_id("quit", "Quit Orbit", true, None),
    ])
    .map_err(|e| e.to_string())?;
    let mut b = tray_icon::TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Orbit");
    // macOS shows the menu on any click (its convention); elsewhere a left
    // click shows or hides Orbit and a right click opens the menu.
    b = b.with_menu_on_left_click(cfg!(target_os = "macos"));
    if let Some(icon) = tray_icon_image() {
        b = b.with_icon(icon);
    }
    b.build().map_err(|e| e.to_string())
}

/// Route tray clicks and menu choices to the window and the UI.
fn install_handlers(ctl: Arc<WinCtl>) {
    use tray_icon::menu::MenuEvent;
    use tray_icon::{MouseButton, MouseButtonState, TrayIconEvent};
    let c = ctl.clone();
    TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = e
        {
            c.toggle();
        }
    }));
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        let cmd = match e.id.0.as_str() {
            "open" => None,
            "search" => Some(TrayCmd::Search),
            "ask" => Some(TrayCmd::Ask),
            "scan" => Some(TrayCmd::Scan),
            "quit" => Some(TrayCmd::Quit),
            _ => return,
        };
        // The UI only runs while the window is shown, so show it first.
        ctl.show();
        if let Some(cmd) = cmd {
            ctl.send(Event::Tray(cmd));
        }
    }));
}

/// Owns the tray icon and the hotkey manager (both must stay alive).
#[derive(Default)]
pub(super) struct TrayParts {
    #[cfg(not(target_os = "linux"))]
    icon: Option<tray_icon::TrayIcon>,
    hotkeys: Option<global_hotkey::GlobalHotKeyManager>,
    hotkey: Option<global_hotkey::hotkey::HotKey>,
}

/// Is there a tray (StatusNotifier host) on this Linux desktop?
#[cfg(target_os = "linux")]
fn linux_has_tray_host() -> bool {
    // For testing on machines without a tray host (CI, headless X).
    if std::env::var("ORBIT_ASSUME_TRAY")
        .map(|v| v == "1")
        .unwrap_or(false)
    {
        return true;
    }
    let out = std::process::Command::new("dbus-send")
        .args([
            "--session",
            "--dest=org.freedesktop.DBus",
            "--type=method_call",
            "--print-reply",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus.NameHasOwner",
            "string:org.kde.StatusNotifierWatcher",
        ])
        .output();
    matches!(out, Ok(o) if String::from_utf8_lossy(&o.stdout).contains("boolean true"))
}

/// Create the tray icon. Call from the first frame (the main thread).
pub(super) fn start_tray(ctl: &Arc<WinCtl>) -> TrayParts {
    #[allow(unused_mut)]
    let mut parts = TrayParts::default();
    install_handlers(ctl.clone());
    #[cfg(target_os = "linux")]
    {
        // GTK needs its own thread and main loop on Linux.
        let c = ctl.clone();
        std::thread::spawn(move || {
            if !linux_has_tray_host() || gtk::init().is_err() {
                c.send(Event::TrayStatus(false));
                return;
            }
            match build_tray() {
                Ok(icon) => {
                    c.tray_ok.store(true, Ordering::Relaxed);
                    c.send(Event::TrayStatus(true));
                    let _keep = icon;
                    gtk::main();
                }
                Err(_) => c.send(Event::TrayStatus(false)),
            }
        });
    }
    #[cfg(not(target_os = "linux"))]
    match build_tray() {
        Ok(icon) => {
            parts.icon = Some(icon);
            ctl.tray_ok.store(true, Ordering::Relaxed);
            ctl.send(Event::TrayStatus(true));
        }
        Err(_) => ctl.send(Event::TrayStatus(false)),
    }
    parts
}

impl TrayParts {
    /// Register (or remove) the show/hide shortcut.
    pub fn set_hotkey(&mut self, ctl: &Arc<WinCtl>, on: bool) -> Result<(), String> {
        use global_hotkey::hotkey::{Code, HotKey, Modifiers};
        use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
        if !on {
            if let (Some(m), Some(h)) = (self.hotkeys.as_ref(), self.hotkey.take()) {
                let _ = m.unregister(h);
            }
            return Ok(());
        }
        if self.hotkey.is_some() {
            return Ok(());
        }
        if self.hotkeys.is_none() {
            self.hotkeys = Some(GlobalHotKeyManager::new().map_err(|e| e.to_string())?);
            let c = ctl.clone();
            GlobalHotKeyEvent::set_event_handler(Some(move |e: GlobalHotKeyEvent| {
                if e.state == HotKeyState::Pressed {
                    c.toggle();
                }
            }));
        }
        let mods = if cfg!(target_os = "macos") {
            Modifiers::META | Modifiers::ALT
        } else {
            Modifiers::CONTROL | Modifiers::ALT
        };
        let key = HotKey::new(Some(mods), Code::KeyO);
        self.hotkeys
            .as_ref()
            .unwrap()
            .register(key)
            .map_err(|e| e.to_string())?;
        self.hotkey = Some(key);
        Ok(())
    }
}

/// The show/hide shortcut as people press it on this OS.
pub(super) fn hotkey_label() -> &'static str {
    if cfg!(target_os = "macos") {
        "Cmd+Option+O"
    } else {
        "Ctrl+Alt+O"
    }
}

/// A native OS notification (toast / Notification Center / D-Bus).
pub(super) fn notify_native(title: &str, body: &str) {
    let (t, b) = (title.to_string(), body.to_string());
    std::thread::spawn(move || {
        let _ = notify_rust::Notification::new()
            .appname("Orbit")
            .summary(&t)
            .body(&b)
            .show();
    });
}
