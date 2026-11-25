//! Turn start-at-sign-in on and off and show what it wrote:
//! `cargo run -p orbit-core --example autostart_check`
fn main() {
    println!("enabled before: {}", orbit_core::autostart::is_enabled());
    orbit_core::autostart::set_enabled(true).unwrap();
    println!("enabled after on: {}", orbit_core::autostart::is_enabled());
    if let Some(h) = dirs::home_dir() {
        if let Ok(s) = std::fs::read_to_string(h.join(".config/autostart/OrbitOS.desktop")) {
            println!("--- ~/.config/autostart/OrbitOS.desktop ---\n{s}");
        }
    }
    orbit_core::autostart::set_enabled(false).unwrap();
    println!("enabled after off: {}", orbit_core::autostart::is_enabled());
}
