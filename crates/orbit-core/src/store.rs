//! App store: one-click installs of free, open-source apps using each OS's
//! package manager (winget on Windows, Homebrew on macOS, Flatpak or apt on
//! Linux).

#[derive(Debug, Clone)]
pub struct StoreApp {
    pub name: &'static str,
    pub category: &'static str,
    pub about: &'static str,
    pub homepage: &'static str,
    pub winget: Option<&'static str>,
    /// (formula or cask name, is_cask)
    pub brew: Option<(&'static str, bool)>,
    pub apt: Option<&'static str>,
    pub flatpak: Option<&'static str>,
    /// A command that works the same everywhere (e.g. npm).
    pub universal: Option<&'static str>,
    /// Programs whose presence means it's installed.
    pub bins: &'static [&'static str],
    /// macOS app bundle name.
    pub mac_app: Option<&'static str>,
}

macro_rules! app {
    ($name:expr, $cat:expr, $about:expr, $home:expr, winget: $w:expr, brew: $b:expr, apt: $a:expr, flatpak: $f:expr, bins: $bins:expr, mac: $mac:expr) => {
        StoreApp {
            name: $name,
            category: $cat,
            about: $about,
            homepage: $home,
            winget: $w,
            brew: $b,
            apt: $a,
            flatpak: $f,
            universal: None,
            bins: $bins,
            mac_app: $mac,
        }
    };
}

pub fn catalog() -> Vec<StoreApp> {
    vec![
        app!("Firefox", "Internet", "Fast, private web browser", "https://www.mozilla.org/firefox/",
            winget: Some("Mozilla.Firefox"), brew: Some(("firefox", true)), apt: Some("firefox"), flatpak: Some("org.mozilla.firefox"), bins: &["firefox"], mac: Some("Firefox")),
        app!("Thunderbird", "Internet", "Email, calendar and contacts", "https://www.thunderbird.net/",
            winget: Some("Mozilla.Thunderbird"), brew: Some(("thunderbird", true)), apt: Some("thunderbird"), flatpak: Some("org.mozilla.Thunderbird"), bins: &["thunderbird"], mac: Some("Thunderbird")),
        app!("mpv", "Media", "The best free video player; OrbitOS Media uses it", "https://mpv.io/",
            winget: Some("mpv.net"), brew: Some(("mpv", false)), apt: Some("mpv"), flatpak: Some("io.mpv.Mpv"), bins: &["mpv", "mpvnet"], mac: None),
        app!("VLC", "Media", "Plays almost any video or audio file", "https://www.videolan.org/vlc/",
            winget: Some("VideoLAN.VLC"), brew: Some(("vlc", true)), apt: Some("vlc"), flatpak: Some("org.videolan.VLC"), bins: &["vlc"], mac: Some("VLC")),
        app!("Audacity", "Media", "Record and edit audio", "https://www.audacityteam.org/",
            winget: Some("Audacity.Audacity"), brew: Some(("audacity", true)), apt: Some("audacity"), flatpak: Some("org.audacityteam.Audacity"), bins: &["audacity"], mac: Some("Audacity")),
        app!("OBS Studio", "Media", "Screen recording and live streaming", "https://obsproject.com/",
            winget: Some("OBSProject.OBSStudio"), brew: Some(("obs", true)), apt: Some("obs-studio"), flatpak: Some("com.obsproject.Studio"), bins: &["obs"], mac: Some("OBS")),
        app!("Kdenlive", "Media", "Video editor", "https://kdenlive.org/",
            winget: Some("KDE.Kdenlive"), brew: Some(("kdenlive", true)), apt: Some("kdenlive"), flatpak: Some("org.kde.kdenlive"), bins: &["kdenlive"], mac: Some("kdenlive")),
        app!("HandBrake", "Media", "Convert and compress videos", "https://handbrake.fr/",
            winget: Some("HandBrake.HandBrake"), brew: Some(("handbrake", true)), apt: Some("handbrake"), flatpak: Some("fr.handbrake.ghb"), bins: &["ghb", "HandBrakeCLI"], mac: Some("HandBrake")),
        app!("LibreOffice", "Office", "Documents, spreadsheets, presentations; OrbitOS Office uses it", "https://www.libreoffice.org/",
            winget: Some("TheDocumentFoundation.LibreOffice"), brew: Some(("libreoffice", true)), apt: Some("libreoffice"), flatpak: Some("org.libreoffice.LibreOffice"), bins: &["soffice", "libreoffice"], mac: Some("LibreOffice")),
        app!("GIMP", "Graphics", "Photo editing", "https://www.gimp.org/",
            winget: Some("GIMP.GIMP"), brew: Some(("gimp", true)), apt: Some("gimp"), flatpak: Some("org.gimp.GIMP"), bins: &["gimp"], mac: Some("GIMP")),
        app!("Inkscape", "Graphics", "Vector graphics and illustration", "https://inkscape.org/",
            winget: Some("Inkscape.Inkscape"), brew: Some(("inkscape", true)), apt: Some("inkscape"), flatpak: Some("org.inkscape.Inkscape"), bins: &["inkscape"], mac: Some("Inkscape")),
        app!("Krita", "Graphics", "Digital painting", "https://krita.org/",
            winget: Some("KDE.Krita"), brew: Some(("krita", true)), apt: Some("krita"), flatpak: Some("org.kde.krita"), bins: &["krita"], mac: Some("krita")),
        app!("Blender", "Engineering & Space", "3D modelling, rendering and animation", "https://www.blender.org/",
            winget: Some("BlenderFoundation.Blender"), brew: Some(("blender", true)), apt: Some("blender"), flatpak: Some("org.blender.Blender"), bins: &["blender"], mac: Some("Blender")),
        app!("FreeCAD", "Engineering & Space", "Parametric 3D CAD for engineering", "https://www.freecad.org/",
            winget: Some("FreeCAD.FreeCAD"), brew: Some(("freecad", true)), apt: Some("freecad"), flatpak: Some("org.freecad.FreeCAD"), bins: &["freecad", "FreeCAD"], mac: Some("FreeCAD")),
        app!("OpenSCAD", "Engineering & Space", "Code-based 3D CAD", "https://openscad.org/",
            winget: Some("OpenSCAD.OpenSCAD"), brew: Some(("openscad", true)), apt: Some("openscad"), flatpak: Some("org.openscad.OpenSCAD"), bins: &["openscad"], mac: Some("OpenSCAD")),
        app!("KiCad", "Engineering & Space", "Electronics and PCB design", "https://www.kicad.org/",
            winget: Some("KiCad.KiCad"), brew: Some(("kicad", true)), apt: Some("kicad"), flatpak: Some("org.kicad.KiCad"), bins: &["kicad"], mac: Some("KiCad")),
        app!("Stellarium", "Engineering & Space", "Realistic planetarium of the night sky", "https://stellarium.org/",
            winget: Some("Stellarium.Stellarium"), brew: Some(("stellarium", true)), apt: Some("stellarium"), flatpak: Some("org.stellarium.Stellarium"), bins: &["stellarium"], mac: Some("Stellarium")),
        app!("ClamAV", "Security", "Open-source antivirus engine; Orbit Shield uses it", "https://www.clamav.net/",
            winget: Some("Cisco.ClamAV"), brew: Some(("clamav", false)), apt: Some("clamav"), flatpak: None, bins: &["clamscan"], mac: None),
        app!("YARA", "Security", "Malware pattern rules; Orbit Shield uses them", "https://virustotal.github.io/yara/",
            winget: Some("VirusTotal.YARA"), brew: Some(("yara", false)), apt: Some("yara"), flatpak: None, bins: &["yara", "yara64"], mac: None),
        app!("KeePassXC", "Security", "Password manager", "https://keepassxc.org/",
            winget: Some("KeePassXCTeam.KeePassXC"), brew: Some(("keepassxc", true)), apt: Some("keepassxc"), flatpak: Some("org.keepassxc.KeePassXC"), bins: &["keepassxc"], mac: Some("KeePassXC")),
        app!("7-Zip", "Utilities", "Open and create zip, 7z and rar archives", "https://www.7-zip.org/",
            winget: Some("7zip.7zip"), brew: Some(("sevenzip", false)), apt: Some("7zip"), flatpak: None, bins: &["7z", "7zz"], mac: None),
        app!("espeak-ng", "Utilities", "Speech for OrbitOS voice replies on Linux", "https://github.com/espeak-ng/espeak-ng",
            winget: None, brew: None, apt: Some("espeak-ng"), flatpak: None, bins: &["espeak-ng"], mac: None),
        app!("Git", "Developer", "Version control", "https://git-scm.com/",
            winget: Some("Git.Git"), brew: Some(("git", false)), apt: Some("git"), flatpak: None, bins: &["git"], mac: None),
        app!("VSCodium", "Developer", "Open-source build of VS Code", "https://vscodium.com/",
            winget: Some("VSCodium.VSCodium"), brew: Some(("vscodium", true)), apt: None, flatpak: Some("com.vscodium.codium"), bins: &["codium"], mac: Some("VSCodium")),
        app!("Godot", "Developer", "Game engine", "https://godotengine.org/",
            winget: Some("GodotEngine.GodotEngine"), brew: Some(("godot", true)), apt: None, flatpak: Some("org.godotengine.Godot"), bins: &["godot"], mac: Some("Godot")),
        app!("Python", "Developer", "Python 3 programming language", "https://www.python.org/",
            winget: Some("Python.Python.3.12"), brew: Some(("python", false)), apt: Some("python3"), flatpak: None, bins: &["python3", "python"], mac: None),
        app!("Node.js", "Developer", "JavaScript runtime (needed for Claude Code)", "https://nodejs.org/",
            winget: Some("OpenJS.NodeJS.LTS"), brew: Some(("node", false)), apt: Some("nodejs"), flatpak: None, bins: &["node"], mac: None),
        app!("Rust", "Developer", "The language OrbitOS is written in (rustup)", "https://rustup.rs/",
            winget: Some("Rustlang.Rustup"), brew: Some(("rustup", false)), apt: Some("rustup"), flatpak: None, bins: &["cargo", "rustup"], mac: None),
        app!("whisper.cpp", "AI", "Offline speech-to-text for OrbitOS voice", "https://github.com/ggerganov/whisper.cpp",
            winget: None, brew: Some(("whisper-cpp", false)), apt: None, flatpak: None, bins: &["whisper-cli", "whisper-cpp"], mac: None),
        StoreApp {
            name: "Claude Code",
            category: "AI",
            about: "Claude in your terminal; OrbitOS's default Claude connection",
            homepage: "https://claude.com/claude-code",
            winget: None,
            brew: None,
            apt: None,
            flatpak: None,
            universal: Some("npm install -g @anthropic-ai/claude-code"),
            bins: &["claude"],
            mac_app: None,
        },
    ]
}

pub fn categories() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = catalog().iter().map(|a| a.category).collect();
    v.dedup();
    v.sort();
    v.dedup();
    v
}

pub fn is_installed(app: &StoreApp) -> bool {
    if app.bins.iter().any(|b| crate::exec::which(b).is_some()) {
        return true;
    }
    if cfg!(target_os = "macos") {
        if let Some(name) = app.mac_app {
            return std::path::Path::new(&format!("/Applications/{name}.app")).exists();
        }
    }
    if cfg!(target_os = "linux") {
        if let Some(id) = app.flatpak {
            return std::path::Path::new(&format!("/var/lib/flatpak/app/{id}")).exists()
                || dirs::home_dir()
                    .map(|h| h.join(format!(".local/share/flatpak/app/{id}")).exists())
                    .unwrap_or(false);
        }
    }
    if cfg!(windows) {
        // Start Menu shortcut with the app's name.
        return crate::apps::find(&crate::apps::discover(), app.name)
            .iter()
            .any(|a| a.name.eq_ignore_ascii_case(app.name));
    }
    false
}

/// The install command for this OS, or `None` if there's no package.
pub fn install_command(app: &StoreApp) -> Option<String> {
    if let Some(u) = app.universal {
        return Some(u.to_string());
    }
    if cfg!(windows) {
        app.winget.map(|id| format!("winget install -e --id {id} --accept-package-agreements --accept-source-agreements"))
    } else if cfg!(target_os = "macos") {
        app.brew.map(|(n, cask)| {
            if cask {
                format!("brew install --cask {n}")
            } else {
                format!("brew install {n}")
            }
        })
    } else if let (Some(id), true) = (app.flatpak, crate::exec::which("flatpak").is_some()) {
        Some(format!("flatpak install -y --user flathub {id}"))
    } else {
        app.apt.map(|p| format!("pkexec apt-get install -y {p}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_sane() {
        let c = catalog();
        assert!(c.len() >= 25);
        for a in &c {
            assert!(!a.bins.is_empty(), "{}", a.name);
            assert!(a.homepage.starts_with("https://"));
        }
        let claude = c.iter().find(|a| a.name == "Claude Code").unwrap();
        assert!(install_command(claude).unwrap().contains("npm"));
        assert!(categories().contains(&"Engineering & Space"));
    }
}
