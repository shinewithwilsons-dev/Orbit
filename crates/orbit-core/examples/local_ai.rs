//! Talk to a local OpenAI-compatible server (vLLM etc.):
//! `cargo run -p orbit-core --example local_ai -- http://localhost:8000/v1`
use orbit_core::{claude, BackendKind, Config, Message};

fn main() {
    let mut cfg = Config::default();
    cfg.backend = BackendKind::Local;
    if let Some(url) = std::env::args().nth(1) {
        cfg.local_url = url;
    }
    println!("models: {:?}", claude::list_local_models(&cfg));
    let reply = claude::ask(&cfg, &[Message::user("hi")], &mut |c| print!("[{c}]"));
    println!("\nreply: {reply:?}");
    println!(
        "image: {:?}",
        claude::generate_image(&cfg, "a warp-lifter over Coventry").map_err(|e| e.to_string())
    );
}
