//! `orbit`: the OrbitOS terminal. Built-in commands, Claude chat, and
//! everything else passed through to the host shell.

use orbit_core::{
    antivirus, apps, claude, exec, files, human_bytes, media, office, system, BackendKind, Config,
    Message,
};
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use std::io::Write;
use std::path::PathBuf;

const CYAN: &str = "\x1b[36m";
const MAGENTA: &str = "\x1b[35m";
const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const RED: &str = "\x1b[31m";
const RESET: &str = "\x1b[0m";

struct Shell {
    cfg: Config,
    cwd: PathBuf,
    chat: Vec<Message>,
    monitor: system::Monitor,
    apps: Option<Vec<apps::App>>,
    editor: DefaultEditor,
}

fn main() -> anyhow::Result<()> {
    // `orbit ask what is my IP` runs one command and exits.
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut shell = Shell {
        cfg: Config::load(),
        cwd: std::env::current_dir().unwrap_or_else(|_| ".".into()),
        chat: Vec::new(),
        monitor: system::Monitor::new(),
        apps: None,
        editor: DefaultEditor::new()?,
    };
    if !args.is_empty() {
        shell.dispatch(&args.join(" "));
        return Ok(());
    }

    banner(&shell.cfg);
    let history = Config::path().with_file_name("history.txt");
    let _ = shell.editor.load_history(&history);

    loop {
        let prompt = format!(
            "{CYAN}orbit{RESET}:{DIM}{}{RESET}> ",
            short_path(&shell.cwd)
        );
        match shell.editor.readline(&prompt) {
            Ok(line) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let _ = shell.editor.add_history_entry(line);
                if matches!(line, "exit" | "quit" | "logout") {
                    break;
                }
                shell.dispatch(line);
            }
            Err(ReadlineError::Interrupted) => continue,
            Err(ReadlineError::Eof) => break,
            Err(e) => {
                eprintln!("{RED}input error: {e}{RESET}");
                break;
            }
        }
    }
    if let Some(dir) = history.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = shell.editor.save_history(&history);
    Ok(())
}

fn banner(cfg: &Config) {
    println!(
        "{MAGENTA}{BOLD}\n   OrbitOS shell {}{RESET}  {DIM}on {} · Claude via {}{RESET}\n   {DIM}Type `help` for commands, `? <question>` to ask Claude.{RESET}\n",
        env!("CARGO_PKG_VERSION"),
        orbit_core::host_os(),
        claude::backend_status(cfg),
    );
}

fn short_path(p: &std::path::Path) -> String {
    if let Some(home) = dirs_home() {
        if let Ok(rest) = p.strip_prefix(&home) {
            return format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display())
                .trim_end_matches(std::path::MAIN_SEPARATOR)
                .to_string();
        }
    }
    p.display().to_string()
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

impl Shell {
    fn dispatch(&mut self, line: &str) {
        let (cmd, rest) = match line.split_once(char::is_whitespace) {
            Some((c, r)) => (c, r.trim()),
            None => (line, ""),
        };
        let result = match cmd {
            "help" => {
                help();
                Ok(())
            }
            "?" | "ask" => self.ask(rest),
            "do" => self.do_task(rest),
            "explain" => self.explain(rest),
            "forget" | "newchat" => {
                self.chat.clear();
                println!("{DIM}Conversation cleared.{RESET}");
                Ok(())
            }
            "scan" => self.scan(rest),
            "quarantine" => self.quarantine_list(),
            "restore" => antivirus::restore(rest).map(|p| println!("Restored {}", p.display())),
            "update-av" => antivirus::update_clamav().map(|m| println!("ClamAV: {m}")),
            "play" => self.play(rest),
            "music" => self.library(media::Kind::Audio, rest),
            "videos" => self.library(media::Kind::Video, rest),
            "office" => self.office(rest),
            "pdf" => office::convert(&self.resolve(rest), "pdf")
                .map(|p| println!("Saved {}", p.display())),
            "draft" => self.draft(rest),
            "image" => claude::generate_image(&self.cfg, rest).map(|p| {
                println!("Saved {}", p.display());
                let _ = exec::open(&p.to_string_lossy());
            }),
            "local-url" => {
                self.cfg.local_url = rest.to_string();
                self.cfg
                    .save()
                    .map(|_| println!("Local server set to {rest}"))
            }
            "install" => self.install(rest),
            "backup" => {
                let pw = self
                    .editor
                    .readline("Backup password (empty = no encryption, secrets left out): ")
                    .unwrap_or_default();
                let dir = orbit_core::backup::suggested_dirs()
                    .into_iter()
                    .next()
                    .map(|(_, p)| p)
                    .unwrap_or_else(|| ".".into());
                orbit_core::backup::create(&dir, pw.trim())
                    .map(|p| println!("Saved {}", p.display()))
            }
            "calc" | "=" => match orbit_core::calc::eval(rest) {
                Some(v) => {
                    println!("{}", orbit_core::calc::format(v));
                    Ok(())
                }
                None => Err(anyhow::anyhow!("couldn't work that out")),
            },
            "radio" => self.radio(rest),
            "cd" => self.cd(rest),
            "pwd" => {
                println!("{}", self.cwd.display());
                Ok(())
            }
            "ls" | "dir" if !rest.starts_with('-') => self.ls(rest),
            "cat" if !rest.is_empty() && !rest.contains(' ') => self.cat(rest),
            "sys" => {
                print!("{}", self.monitor.snapshot(0).summary());
                Ok(())
            }
            "ps" | "top" => self.ps(),
            "kill" => self.kill(rest),
            "apps" => self.list_apps(rest),
            "launch" => self.launch(rest),
            "open" => exec::open(if rest.is_empty() { "." } else { rest }),
            "backend" => self.backend(rest),
            "model" => self.model(rest),
            "key" => self.set_key(),
            "config" => {
                println!("{}\n{}", Config::path().display(), toml_view(&self.cfg));
                Ok(())
            }
            "clear" | "cls" => {
                print!("\x1b[2J\x1b[H");
                Ok(())
            }
            _ => self.passthrough(line),
        };
        if let Err(e) = result {
            eprintln!("{RED}{e}{RESET}");
        }
    }

    fn passthrough(&self, line: &str) -> anyhow::Result<()> {
        let status = exec::run_interactive(line, &self.cwd)?;
        if !status.success() {
            println!("{DIM}exit: {status}{RESET}");
        }
        Ok(())
    }

    // ---------------------------------------------------------- Claude --

    fn stream(&mut self, prompt: String) -> anyhow::Result<String> {
        self.chat.push(Message::user(prompt));
        println!(
            "{MAGENTA}{BOLD}Claude{RESET} {DIM}({}){RESET}",
            claude::backend_status(&self.cfg)
        );
        let _ = std::env::set_current_dir(&self.cwd);
        let reply = claude::ask(&self.cfg, &self.chat, &mut |chunk| {
            print!("{chunk}");
            let _ = std::io::stdout().flush();
        });
        println!();
        match reply {
            Ok(text) => {
                self.chat.push(Message::assistant(text.clone()));
                Ok(text)
            }
            Err(e) => {
                self.chat.pop();
                Err(e)
            }
        }
    }

    fn ask(&mut self, q: &str) -> anyhow::Result<()> {
        if q.is_empty() {
            anyhow::bail!("usage: ? <question>");
        }
        let reply = self.stream(q.to_string())?;
        self.offer_commands(&reply)
    }

    fn do_task(&mut self, task: &str) -> anyhow::Result<()> {
        if task.is_empty() {
            anyhow::bail!("usage: do <what you want done>");
        }
        let prompt = format!(
            "Give me one {} command, run from {}, that does this: {task}\n\
             Put the command in a ```run block, then one line explaining it.",
            orbit_core::host_shell(),
            self.cwd.display()
        );
        let reply = self.stream(prompt)?;
        self.offer_commands(&reply)
    }

    fn explain(&mut self, file: &str) -> anyhow::Result<()> {
        if file.is_empty() {
            anyhow::bail!("usage: explain <file>");
        }
        let path = self.resolve(file);
        let text = files::read_text_preview(&path, 24_000)
            .ok_or_else(|| anyhow::anyhow!("{} is not a readable text file", path.display()))?;
        self.stream(format!(
            "Explain this file ({}) briefly and point out anything wrong:\n\n{text}",
            path.display()
        ))?;
        Ok(())
    }

    fn offer_commands(&mut self, reply: &str) -> anyhow::Result<()> {
        for cmd in claude::extract_commands(reply) {
            println!("\n{BOLD}Suggested:{RESET} {CYAN}{cmd}{RESET}");
            let answer = self.editor.readline("Run it? [y/N] ").unwrap_or_default();
            if answer.trim().eq_ignore_ascii_case("y") {
                if let Some(dir) = cmd.strip_prefix("cd ") {
                    self.cd(dir.trim())?;
                } else {
                    self.passthrough(&cmd)?;
                }
            }
        }
        // Claude actions: approve each one.
        let mut results = Vec::new();
        for a in orbit_core::actions::extract(reply) {
            match a {
                Ok(action) => {
                    println!(
                        "\n{BOLD}Claude wants to:{RESET} {CYAN}{}{RESET}  {}",
                        action.title(),
                        action.detail()
                    );
                    let answer = self.editor.readline("Approve? [y/N] ").unwrap_or_default();
                    if answer.trim().eq_ignore_ascii_case("y") {
                        let out = orbit_core::actions::execute(&action, &self.cfg, &self.cwd);
                        println!("{}{}{RESET}", if out.ok { DIM } else { RED }, out.message);
                        results.push(format!(
                            "- {}: {} -> {}",
                            action.title(),
                            action.detail(),
                            out.message
                        ));
                    } else {
                        results.push(format!(
                            "- {}: {} -> declined",
                            action.title(),
                            action.detail()
                        ));
                    }
                }
                Err(e) => println!("{RED}{e}{RESET}"),
            }
        }
        if !results.is_empty() {
            let more = self
                .editor
                .readline("Send the results back to Claude? [Y/n] ")
                .unwrap_or_default();
            if !more.trim().eq_ignore_ascii_case("n") {
                let reply = self.stream(format!(
                    "Results of the actions I approved:\n{}",
                    results.join("\n")
                ))?;
                return self.offer_commands(&reply);
            }
        }
        Ok(())
    }

    fn backend(&mut self, arg: &str) -> anyhow::Result<()> {
        if arg.is_empty() {
            println!(
                "Backend: {} (resolved: {}). Options: cli | api | local | auto",
                self.cfg.backend.label(),
                claude::backend_status(&self.cfg)
            );
            return Ok(());
        }
        let kind = BackendKind::parse(arg)
            .ok_or_else(|| anyhow::anyhow!("use cli, api, local or auto"))?;
        if kind == BackendKind::Local {
            match claude::list_local_models(&self.cfg) {
                Ok(m) => println!(
                    "{DIM}Local server {} has: {}{RESET}",
                    self.cfg.local_url,
                    m.join(", ")
                ),
                Err(e) => println!("{DIM}Warning: {e} (set the URL with `local-url <url>`){RESET}"),
            }
        }
        if kind == BackendKind::Api && self.cfg.effective_api_key().is_none() {
            println!("{DIM}No API key yet: run `key` or set ANTHROPIC_API_KEY.{RESET}");
        }
        self.cfg.backend = kind;
        self.cfg.save()?;
        println!("Claude backend set to {}.", kind.label());
        Ok(())
    }

    fn model(&mut self, arg: &str) -> anyhow::Result<()> {
        if arg.is_empty() {
            println!("API model: {}", self.cfg.model);
        } else {
            self.cfg.model = arg.to_string();
            self.cfg.save()?;
            println!("API model set to {arg}.");
        }
        Ok(())
    }

    fn set_key(&mut self) -> anyhow::Result<()> {
        println!(
            "{DIM}The key is stored in plain text at {}. Leave empty to remove it.{RESET}",
            Config::path().display()
        );
        let k = self.editor.readline("Anthropic API key: ")?;
        let k = k.trim();
        self.cfg.api_key = if k.is_empty() {
            None
        } else {
            Some(k.to_string())
        };
        self.cfg.save()?;
        println!("Saved.");
        Ok(())
    }

    // ----------------------------------------------------- Orbit Shield --

    fn scan(&mut self, target: &str) -> anyhow::Result<()> {
        let targets = if target.is_empty() {
            antivirus::quick_scan_paths()
        } else {
            vec![self.resolve(target)]
        };
        println!(
            "{BOLD}Orbit Shield{RESET} scanning: {}",
            targets
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
        let opts = antivirus::ScanOptions {
            use_clamav: self.cfg.use_clamav,
            ..Default::default()
        };
        let report = antivirus::scan(
            &targets,
            &opts,
            &std::sync::atomic::AtomicBool::new(false),
            &mut |n, _| {
                print!("\r{DIM}{n} files checked…{RESET}");
                let _ = std::io::stdout().flush();
            },
        );
        println!(
            "\r{} files checked in {:.1}s{}.          ",
            report.files_scanned,
            report.duration.as_secs_f32(),
            if report.clamav_used {
                " (Orbit engine + ClamAV)"
            } else {
                " (Orbit engine)"
            }
        );
        for n in &report.notes {
            println!("{DIM}{n}{RESET}");
        }
        if report.findings.is_empty() {
            println!("\x1b[32mNo threats found.{RESET}");
            return Ok(());
        }
        for f in &report.findings {
            let col = match f.severity {
                antivirus::Severity::High => RED,
                antivirus::Severity::Medium => "\x1b[33m",
                antivirus::Severity::Low => DIM,
            };
            println!(
                "\n{col}{BOLD}[{}]{RESET} {BOLD}{}{RESET} {DIM}({}){RESET}\n  {}\n  {DIM}{}{RESET}",
                f.severity.label(),
                f.threat,
                f.engine,
                f.path.display(),
                f.detail
            );
            let ans = self
                .editor
                .readline("  Quarantine [q], ask Claude [c], or skip [Enter]? ")
                .unwrap_or_default();
            match ans.trim() {
                "q" | "Q" => match antivirus::quarantine(&f.path, &f.threat) {
                    Ok(_) => println!("  Quarantined."),
                    Err(e) => println!("  {RED}{e}{RESET}"),
                },
                "c" | "C" => {
                    self.stream(f.describe())?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn quarantine_list(&self) -> anyhow::Result<()> {
        let items = antivirus::quarantine_list();
        if items.is_empty() {
            println!("Quarantine is empty.");
        }
        for q in items {
            println!(
                "{BOLD}{}{RESET}  {}  {DIM}{}{RESET}",
                q.id,
                q.threat,
                q.original.display()
            );
        }
        println!("{DIM}Use `restore <id>` to put a file back.{RESET}");
        Ok(())
    }

    // ------------------------------------------------------- media/office --

    fn play(&self, target: &str) -> anyhow::Result<()> {
        if target.is_empty() {
            anyhow::bail!("usage: play <file>   (music and video open in mpv, then VLC)");
        }
        let by = media::play_external(&[self.resolve(target)])?;
        println!("Playing in {by}.");
        Ok(())
    }

    fn library(&self, kind: media::Kind, filter: &str) -> anyhow::Result<()> {
        let items = media::scan(&media::library_dirs(kind), kind);
        let q = filter.to_lowercase();
        let found: Vec<_> = items
            .iter()
            .filter(|i| i.title.to_lowercase().contains(&q))
            .collect();
        for i in &found {
            println!(
                "  {:<40} {DIM}{}{RESET}",
                truncate(&i.title, 40),
                i.path.display()
            );
        }
        println!("{DIM}{} items · `play <path>` to play{RESET}", found.len());
        if media::players().is_empty() {
            println!(
                "{DIM}No mpv/VLC found: {}{RESET}",
                media::player_install_hint()
            );
        }
        Ok(())
    }

    fn office(&self, args: &str) -> anyhow::Result<()> {
        let (sub, rest) = args
            .split_once(' ')
            .map(|(a, b)| (a, b.trim()))
            .unwrap_or((args, ""));
        match sub {
            "new" => {
                let kind = office::DocKind::parse(if rest.is_empty() { "writer" } else { rest })
                    .ok_or_else(|| anyhow::anyhow!("use: office new writer|calc|impress|draw"))?;
                office::new_document(kind)?;
                println!("Opening LibreOffice {}.", kind.app());
            }
            "open" => office::open(&self.resolve(rest))?,
            "recent" | "" => {
                match office::find_soffice() {
                    Some(p) => println!("LibreOffice: {}", p.display()),
                    None => println!("LibreOffice not installed: {}", office::install_hint()),
                }
                for p in office::recent_documents(15) {
                    println!("  {}", p.display());
                }
                println!("{DIM}office new <writer|calc|impress|draw> · office open <file> · pdf <file> · draft <topic>{RESET}");
            }
            _ => anyhow::bail!("use: office [new <kind> | open <file> | recent]"),
        }
        Ok(())
    }

    fn radio(&mut self, query: &str) -> anyhow::Result<()> {
        let st = if query.is_empty() {
            orbit_core::radio::featured()
        } else {
            orbit_core::radio::search(query)?
        };
        for (i, s) in st.iter().take(15).enumerate() {
            println!(
                "{:>3}. {BOLD}{}{RESET} {DIM}{}{RESET}",
                i + 1,
                s.name,
                s.info
            );
        }
        let pick = self
            .editor
            .readline("Play which number? ")
            .unwrap_or_default();
        if let Some(s) = pick
            .trim()
            .parse::<usize>()
            .ok()
            .and_then(|n| st.get(n.wrapping_sub(1)))
        {
            media::play_external(&[std::path::PathBuf::from(&s.url)])?;
            println!("Playing {} in your media player.", s.name);
        }
        Ok(())
    }

    fn install(&mut self, name: &str) -> anyhow::Result<()> {
        let catalog = orbit_core::store::catalog();
        if name.is_empty() {
            for a in &catalog {
                let mark = if orbit_core::store::is_installed(a) {
                    "\x1b[32minstalled\x1b[0m"
                } else {
                    ""
                };
                println!("  {BOLD}{:<14}{RESET} {:<58} {mark}", a.name, a.about);
            }
            println!("{DIM}install <name> to install one{RESET}");
            return Ok(());
        }
        let app = catalog
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| {
                anyhow::anyhow!("no app called '{name}' (type `install` for the list)")
            })?;
        let cmd = orbit_core::store::install_command(app)
            .ok_or_else(|| anyhow::anyhow!("no package for this system: see {}", app.homepage))?;
        println!("{BOLD}Will run:{RESET} {CYAN}{cmd}{RESET}");
        if self
            .editor
            .readline("Install? [y/N] ")
            .unwrap_or_default()
            .trim()
            .eq_ignore_ascii_case("y")
        {
            self.passthrough(&cmd)?;
        }
        Ok(())
    }

    fn draft(&mut self, topic: &str) -> anyhow::Result<()> {
        if topic.is_empty() {
            anyhow::bail!("usage: draft <what the document should be>");
        }
        let text = self.stream(format!(
            "Write the following as a finished document in Markdown, starting with a '# ' title. \
             Output only the document.\n\nRequest: {topic}"
        ))?;
        let title = text
            .lines()
            .find_map(|l| l.trim().strip_prefix("# ").map(str::to_string))
            .unwrap_or_else(|| topic.to_string());
        let path = office::save_draft(&title, &text, "docx")?;
        println!("Saved {}", path.display());
        let _ = office::open(&path);
        Ok(())
    }

    // ------------------------------------------------------------ files --

    fn resolve(&self, p: &str) -> PathBuf {
        let p = orbit_core::expand_tilde(p);
        if p.is_absolute() {
            p
        } else {
            self.cwd.join(p)
        }
    }

    fn cd(&mut self, dir: &str) -> anyhow::Result<()> {
        let target = if dir.is_empty() {
            dirs_home().unwrap_or_else(|| ".".into())
        } else {
            self.resolve(dir)
        };
        let target = target.canonicalize()?;
        if !target.is_dir() {
            anyhow::bail!("not a directory: {}", target.display());
        }
        self.cwd = target;
        Ok(())
    }

    fn ls(&self, dir: &str) -> anyhow::Result<()> {
        let path = if dir.is_empty() {
            self.cwd.clone()
        } else {
            self.resolve(dir)
        };
        for e in files::list(&path, false)? {
            if e.is_dir {
                println!("{CYAN}{}/{RESET}", e.name);
            } else {
                println!("{:<40} {DIM}{:>10}{RESET}", e.name, human_bytes(e.size));
            }
        }
        Ok(())
    }

    fn cat(&self, file: &str) -> anyhow::Result<()> {
        print!("{}", std::fs::read_to_string(self.resolve(file))?);
        Ok(())
    }

    // ----------------------------------------------------------- system --

    fn ps(&mut self) -> anyhow::Result<()> {
        // Two samples so CPU figures are real.
        self.monitor.snapshot(0);
        std::thread::sleep(std::time::Duration::from_millis(400));
        let s = self.monitor.snapshot(15);
        println!(
            "{BOLD}{:>8}  {:<28} {:>7} {:>10}{RESET}",
            "PID", "NAME", "CPU%", "MEM"
        );
        for p in s.top_procs {
            println!(
                "{:>8}  {:<28} {:>6.1}% {:>10}",
                p.pid,
                truncate(&p.name, 28),
                p.cpu,
                human_bytes(p.memory)
            );
        }
        println!(
            "{DIM}{} processes · CPU {:.0}% · RAM {} / {}{RESET}",
            s.proc_count,
            s.cpu_total,
            human_bytes(s.mem_used),
            human_bytes(s.mem_total)
        );
        Ok(())
    }

    fn kill(&mut self, pid: &str) -> anyhow::Result<()> {
        let pid: u32 = pid
            .parse()
            .map_err(|_| anyhow::anyhow!("usage: kill <pid>"))?;
        let ok = self
            .editor
            .readline(&format!("End process {pid}? [y/N] "))
            .unwrap_or_default();
        if ok.trim().eq_ignore_ascii_case("y") {
            self.monitor.snapshot(0);
            println!(
                "{}",
                if self.monitor.kill(pid) {
                    "Signal sent."
                } else {
                    "Could not end that process."
                }
            );
        }
        Ok(())
    }

    fn apps(&mut self) -> &[apps::App] {
        self.apps.get_or_insert_with(apps::discover)
    }

    fn list_apps(&mut self, filter: &str) -> anyhow::Result<()> {
        let list = self.apps();
        let found = apps::find(list, filter);
        for a in &found {
            println!("  {}", a.name);
        }
        println!("{DIM}{} apps{RESET}", found.len());
        Ok(())
    }

    fn launch(&mut self, name: &str) -> anyhow::Result<()> {
        if name.is_empty() {
            anyhow::bail!("usage: launch <app name>");
        }
        let list = self.apps().to_vec();
        let found = apps::find(&list, name);
        let app = found
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(name))
            .or_else(|| found.first())
            .ok_or_else(|| anyhow::anyhow!("no app matching '{name}' (try `apps`)"))?;
        apps::launch(app)?;
        println!("Launched {}.", app.name);
        Ok(())
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n - 1).collect::<String>())
    }
}

fn toml_view(cfg: &Config) -> String {
    let mut c = cfg.clone();
    if c.api_key.is_some() {
        c.api_key = Some("•••••• (hidden)".into());
    }
    format!(
        "backend = {:?}\nmodel = {}\nmax_tokens = {}\napi_key = {}\ncli_path = {}\nfullscreen = {}",
        c.backend,
        c.model,
        c.max_tokens,
        c.api_key.unwrap_or_else(|| "none".into()),
        c.cli_path.unwrap_or_else(|| "auto".into()),
        c.fullscreen
    )
}

fn help() {
    println!(
        "{BOLD}Claude{RESET}
  ? <question>      ask Claude (conversation is remembered)
  do <task>         Claude writes a command; you confirm before it runs
  explain <file>    Claude explains a text or code file
  forget            start a new conversation
  backend [cli|api|auto]   show or switch how Claude is reached
  model [name]      API model   ·   key   set or remove API key
{BOLD}Files{RESET}
  ls [dir]  cd [dir]  pwd  cat <file>  open [path|url]
{BOLD}Orbit Shield (antivirus){RESET}
  scan [path]       quick scan, or scan a file/folder (Orbit engine + ClamAV)
  quarantine        list quarantined files  ·  restore <id>  ·  update-av
{BOLD}Media & Office{RESET}
  music [filter]  videos [filter]  play <file>      (mpv / VLC)
  office [new <writer|calc|impress|draw> | open <file>]   pdf <file>
  draft <topic>     Claude writes it, saved as a Word document
  radio [search]    internet radio · image <prompt> (local server)
{BOLD}Apps & backup{RESET}
  install [name]    free app store (winget / Homebrew / Flatpak / apt)
  backup            encrypted backup of settings, chats and playlists
  = <maths>         calculator, e.g. `= 3e8 / 1000`
  local-url <url>   your vLLM / OpenAI-compatible server (then `backend local`)
{BOLD}System{RESET}
  sys   ps   kill <pid>   apps [filter]   launch <app>
{BOLD}Other{RESET}
  config  clear  exit
Anything else runs in your host shell ({}).",
        orbit_core::host_shell()
    );
}
