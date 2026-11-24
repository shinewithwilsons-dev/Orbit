//! Claude integration with two switchable backends:
//!
//! * **CLI**: pipes the conversation into the signed-in Claude Code CLI
//!   (`claude -p`). No API key is stored anywhere by OrbitOS.
//! * **API**: streams from the Anthropic Messages API using an API key.

use crate::config::{BackendKind, Config};
use anyhow::{bail, Context, Result};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
destructive commands (deleting, formatting, overwriting) without saying so plainly.\n\n{actions}",
        os = crate::host_os(),
        shell = crate::host_shell(),
        actions = crate::actions::tool_guide(),
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
        BackendKind::Local => ask_local(cfg, history, on_chunk),
        _ => ask_cli(cfg, history, on_chunk),
    }
}

/// Short status line for UIs.
pub fn backend_status(cfg: &Config) -> String {
    match cfg.resolved_backend() {
        BackendKind::Api => format!("API · {}", cfg.model),
        BackendKind::Local => format!(
            "Local · {}",
            if cfg.local_model.is_empty() {
                "auto model"
            } else {
                cfg.local_model.as_str()
            }
        ),
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

// -------------------------------------------------------------- Local ----
// Any OpenAI-compatible server: vLLM (`vllm serve <model>`), Ollama,
// LM Studio, llama.cpp server.

fn local_base(cfg: &Config) -> String {
    cfg.local_url.trim_end_matches('/').to_string()
}

fn local_request(cfg: &Config, method: &str, path: &str) -> ureq::Request {
    let mut r = ureq::request(method, &format!("{}{path}", local_base(cfg)))
        .timeout(std::time::Duration::from_secs(600));
    if let Some(k) = cfg.local_api_key.as_ref().filter(|k| !k.trim().is_empty()) {
        r = r.set("Authorization", &format!("Bearer {k}"));
    }
    r
}

/// Models the local server offers (`GET /v1/models`).
pub fn list_local_models(cfg: &Config) -> Result<Vec<String>> {
    let v: serde_json::Value = local_request(cfg, "GET", "/models")
        .timeout(std::time::Duration::from_secs(5))
        .call()
        .with_context(|| format!("can't reach {}", cfg.local_url))?
        .into_json()?;
    Ok(v["data"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| m["id"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default())
}

fn local_model(cfg: &Config) -> Result<String> {
    if !cfg.local_model.trim().is_empty() {
        return Ok(cfg.local_model.trim().to_string());
    }
    list_local_models(cfg)?
        .into_iter()
        .next()
        .context("the local server didn't list any models")
}

fn ask_local(cfg: &Config, history: &[Message], on_chunk: &mut dyn FnMut(&str)) -> Result<String> {
    let model = local_model(cfg)?;
    let mut messages = vec![serde_json::json!({"role": "system", "content": system_prompt()})];
    messages.extend(
        history
            .iter()
            .filter(|m| !m.content.trim().is_empty())
            .map(|m| {
                serde_json::json!({
                    "role": match m.role { Role::User => "user", Role::Assistant => "assistant" },
                    "content": m.content,
                })
            }),
    );
    let body = serde_json::json!({
        "model": model,
        "messages": messages,
        "max_tokens": cfg.max_tokens,
        "stream": true,
    });
    let resp = match local_request(cfg, "POST", "/chat/completions").send_json(body) {
        Ok(r) => r,
        Err(ureq::Error::Status(code, r)) => {
            bail!(
                "local server error {code}: {}",
                r.into_string().unwrap_or_default()
            )
        }
        Err(e) => bail!("can't reach the local server at {}: {e}", cfg.local_url),
    };
    let mut full = String::new();
    for line in BufReader::new(resp.into_reader()).lines() {
        let line = line?;
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data == "[DONE]" {
            break;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(data) else {
            continue;
        };
        if let Some(t) = v["choices"][0]["delta"]["content"].as_str() {
            on_chunk(t);
            full.push_str(t);
        }
    }
    Ok(full)
}

/// Generate an image on the local server (`POST /v1/images/generations`).
/// Saves it to Pictures/OrbitOS and returns the path.
pub fn generate_image(cfg: &Config, prompt: &str) -> Result<std::path::PathBuf> {
    let mut body = serde_json::json!({
        "prompt": prompt,
        "n": 1,
        "size": "1024x1024",
        "response_format": "b64_json",
    });
    if !cfg.image_model.trim().is_empty() {
        body["model"] = serde_json::json!(cfg.image_model.trim());
    }
    let v: serde_json::Value = match local_request(cfg, "POST", "/images/generations").send_json(body) {
        Ok(r) => r.into_json()?,
        Err(ureq::Error::Status(code, r)) => bail!(
            "image generation failed ({code}): {}. Your server needs an OpenAI-style /v1/images/generations endpoint.",
            r.into_string().unwrap_or_default()
        ),
        Err(e) => bail!("can't reach {}: {e}", cfg.local_url),
    };
    let item = &v["data"][0];
    let bytes = if let Some(b64) = item["b64_json"].as_str() {
        crate::b64_decode(b64).context("bad image data")?
    } else if let Some(url) = item["url"].as_str() {
        let mut buf = Vec::new();
        ureq::get(url).call()?.into_reader().read_to_end(&mut buf)?;
        buf
    } else {
        bail!("the server returned no image");
    };
    let dir = dirs::picture_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Pictures")))
        .unwrap_or_else(|| ".".into())
        .join("OrbitOS");
    std::fs::create_dir_all(&dir)?;
    let name = format!(
        "{} {}.png",
        crate::office::safe_name(&prompt.chars().take(40).collect::<String>()),
        crate::now_secs()
    );
    let path = dir.join(name);
    std::fs::write(&path, bytes)?;
    Ok(path)
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
