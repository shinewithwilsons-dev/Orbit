//! Claude integration with two switchable backends:
//!
//! * **CLI**: pipes the conversation into the signed-in Claude Code CLI
//!   (`claude -p`). No API key is stored anywhere by OrbitOS.
//! * **API**: streams from the Anthropic Messages API using an API key.

use crate::config::{BackendKind, Config};
use anyhow::{bail, Context, Result};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

impl Message {
    pub fn user(s: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: s.into(),
        }
    }
    pub fn assistant(s: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: s.into(),
        }
    }
}

/// The system prompt that tells Claude where it is running.
pub fn system_prompt() -> String {
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    format!(
        "You are Claude, the assistant built into OrbitOS, a Rust desktop environment \
running on {os}. The host shell is {shell}. Current directory: {cwd}.\n\
Be concise and practical. When the user wants something done on their computer, \
propose the exact command inside a fenced code block tagged `run`, for example:\n\
```run\n<one command for {shell}>\n```\n\
OrbitOS always asks the user to confirm before running anything. Never propose \
destructive commands (deleting, formatting, overwriting) without saying so plainly.",
        os = crate::host_os(),
        shell = crate::host_shell(),
    )
}

/// Send the conversation to Claude, calling `on_chunk` as text arrives.
/// Returns the full reply.
pub fn ask(cfg: &Config, history: &[Message], on_chunk: &mut dyn FnMut(&str)) -> Result<String> {
    if history.is_empty() {
        bail!("nothing to send");
    }
    match cfg.resolved_backend() {
        BackendKind::Api => ask_api(cfg, history, on_chunk),
        _ => ask_cli(cfg, history, on_chunk),
    }
}

/// Short status line for UIs.
pub fn backend_status(cfg: &Config) -> String {
    match cfg.resolved_backend() {
        BackendKind::Api => format!("API · {}", cfg.model),
        _ => "Claude Code CLI".to_string(),
    }
}

// ---------------------------------------------------------------- CLI ----

fn render_transcript(history: &[Message]) -> String {
    let mut out = String::new();
    out.push_str("<system>\n");
    out.push_str(&system_prompt());
    out.push_str("\n</system>\n\n");
    for m in history {
        let tag = match m.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        };
        out.push_str(&format!("<{tag}>\n{}\n</{tag}>\n\n", m.content.trim()));
    }
    out
}

fn cli_command(cfg: &Config) -> Command {
    if let Some(path) = &cfg.cli_path {
        return Command::new(path);
    }
    if cfg!(windows) {
        // npm installs `claude.cmd` on Windows, which needs cmd.exe to run.
        let mut c = Command::new("cmd");
        c.args(["/C", "claude"]);
        c
    } else {
        Command::new("claude")
    }
}

fn ask_cli(cfg: &Config, history: &[Message], on_chunk: &mut dyn FnMut(&str)) -> Result<String> {
    let transcript = render_transcript(history);
    let mut child = cli_command(cfg)
        .args([
            "-p",
            "Reply as the assistant to the last user turn of the conversation given on standard input",
            "--output-format",
            "text",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                anyhow::anyhow!(
                    "Claude Code CLI not found. Install it with `npm install -g @anthropic-ai/claude-code`, \
run `claude` once to sign in, or switch the backend to `api` in Settings."
                )
            } else {
                anyhow::anyhow!("could not start the Claude CLI: {e}")
            }
        })?;

    let mut stdin = child.stdin.take().context("no stdin")?;
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(transcript.as_bytes());
    });

    let stdout = child.stdout.take().context("no stdout")?;
    let mut reader = BufReader::new(stdout);
    let mut full = String::new();
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        on_chunk(&line);
        full.push_str(&line);
    }
    let _ = writer.join();

    let mut err = String::new();
    if let Some(mut e) = child.stderr.take() {
        let _ = e.read_to_string(&mut err);
    }
    let status = child.wait()?;
    if !status.success() {
        let detail = if err.trim().is_empty() {
            full.trim()
        } else {
            err.trim()
        };
        let lower = detail.to_lowercase();
        let hint = if lower.contains("login") || lower.contains("auth") {
            " Run `claude` in a terminal once to sign in, or switch to the API backend."
        } else {
            ""
        };
        bail!("Claude CLI failed ({status}): {detail}.{hint}");
    }
    Ok(full.trim_end().to_string())
}

// ---------------------------------------------------------------- API ----

fn ask_api(cfg: &Config, history: &[Message], on_chunk: &mut dyn FnMut(&str)) -> Result<String> {
    let key = cfg.effective_api_key().context(
        "no API key: set ANTHROPIC_API_KEY or add one in Settings, or switch to the CLI backend",
    )?;

    let messages: Vec<serde_json::Value> = history
        .iter()
        .filter(|m| !m.content.trim().is_empty())
        .map(|m| {
            serde_json::json!({
                "role": match m.role { Role::User => "user", Role::Assistant => "assistant" },
                "content": m.content,
            })
        })
        .collect();

    let body = serde_json::json!({
        "model": cfg.model,
        "max_tokens": cfg.max_tokens,
        "system": system_prompt(),
        "stream": true,
        "messages": messages,
    });

    let resp = match ureq::post("https://api.anthropic.com/v1/messages")
        .set("x-api-key", &key)
        .set("anthropic-version", "2023-06-01")
        .set("content-type", "application/json")
        .send_json(body)
    {
        Ok(r) => r,
        Err(ureq::Error::Status(code, r)) => {
            bail!("API error {code}: {}", r.into_string().unwrap_or_default())
        }
        Err(e) => return Err(e.into()),
    };

    let mut full = String::new();
    for line in BufReader::new(resp.into_reader()).lines() {
        let line = line?;
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(data.trim()) else {
            continue;
        };
        match v["type"].as_str() {
            Some("content_block_delta") => {
                if let Some(t) = v["delta"]["text"].as_str() {
                    on_chunk(t);
                    full.push_str(t);
                }
            }
            Some("error") => bail!("API stream error: {}", v["error"]["message"]),
            Some("message_stop") => break,
            _ => {}
        }
    }
    Ok(full)
}

// ------------------------------------------------------------ helpers ----

/// Pull every ```run block out of a reply.
pub fn extract_commands(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("```run") {
        let after = &rest[start + 6..];
        let body_start = after.find('\n').map(|i| i + 1).unwrap_or(after.len());
        let body = &after[body_start..];
        match body.find("```") {
            Some(end) => {
                let cmd = body[..end].trim();
                if !cmd.is_empty() {
                    out.push(cmd.to_string());
                }
                rest = &body[end + 3..];
            }
            None => break,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_run_blocks() {
        let t = "Try this:\n```run\nls -la\n```\nor\n```run\necho hi\n```\n```bash\nnot this\n```";
        assert_eq!(extract_commands(t), vec!["ls -la", "echo hi"]);
    }

    #[test]
    fn ignores_unterminated() {
        assert!(extract_commands("```run\nls").is_empty());
    }

    #[test]
    fn transcript_has_roles() {
        let t = render_transcript(&[Message::user("hi"), Message::assistant("hello")]);
        assert!(t.contains("<user>\nhi\n</user>"));
        assert!(t.contains("<assistant>\nhello\n</assistant>"));
        assert!(t.contains("OrbitOS"));
    }

    #[test]
    fn api_without_key_errors() {
        let mut c = Config::default();
        c.backend = BackendKind::Api;
        if std::env::var("ANTHROPIC_API_KEY").is_err() {
            let r = ask(&c, &[Message::user("hi")], &mut |_| {});
            assert!(r.unwrap_err().to_string().contains("no API key"));
        }
    }
}
