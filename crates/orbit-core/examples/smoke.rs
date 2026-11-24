//! Quick end-to-end check of Orbit Shield, Office and Media on this machine:
//! `cargo run -p orbit-core --example smoke`
use orbit_core::{antivirus, media, office};
use std::sync::atomic::AtomicBool;

fn main() {
    println!("ClamAV: {:?}", antivirus::clamav_version());
    println!(
        "freshclam: {:?}",
        antivirus::update_clamav().map_err(|e| e.to_string())
    );
    let dir = std::env::temp_dir().join("orbit-smoke");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("holiday.jpg.exe"), b"MZ").unwrap();
    std::fs::write(dir.join("readme.txt"), b"hello").unwrap();
    let r = antivirus::scan(
        &[dir.clone()],
        &antivirus::ScanOptions::default(),
        &AtomicBool::new(false),
        &mut |_, _| {},
    );
    println!(
        "scanned {} files, clamav_used={}, notes={:?}",
        r.files_scanned, r.clamav_used, r.notes
    );
    for f in &r.findings {
        println!(
            "  [{}] {} - {} ({})",
            f.severity.label(),
            f.path.display(),
            f.threat,
            f.engine
        );
    }
    println!("LibreOffice: {:?}", office::find_soffice());
    match office::save_draft(
        "Smoke Test Report",
        "# Mission brief\nThe **CCWL** test went *well*.\n\n- Item one\n- Item two\n",
        "docx",
    ) {
        Ok(p) => println!(
            "draft saved: {} ({} bytes)",
            p.display(),
            std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0)
        ),
        Err(e) => println!("draft failed: {e}"),
    }
    println!(
        "players: {:?}",
        media::players().iter().map(|p| p.name).collect::<Vec<_>>()
    );
    #[cfg(feature = "audio")]
    println!(
        "audio device: {}",
        media::AudioPlayer::new(0.5)
            .map(|_| "ok".to_string())
            .unwrap_or_else(|e| e.to_string())
    );
    std::fs::remove_dir_all(dir).ok();
}
