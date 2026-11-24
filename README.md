# OrbitOS

A desktop environment and shell written in Rust, with Claude built in.
One codebase runs on **Windows, Linux and macOS**, on any laptop that can run
a modern OS.

OrbitOS runs *on top of* your existing OS. It doesn't replace the kernel.
That's how it can run on any laptop, launch your existing Windows, Mac or
Linux apps, and use their drivers. On Linux it can also act as a full login
session, so it boots straight into OrbitOS.

![OrbitOS desktop](docs/screenshot.png)

## What's inside

| Crate | Binary | What it does |
|---|---|---|
| `orbit-core` | library | Settings, Claude backends, system info, app discovery, files, command execution |
| `orbit-shell` | `orbit` | Terminal shell: built-ins, Claude chat, `do` tasks, passthrough to the host shell |
| `orbit-desktop` | `orbit-desktop` | GUI desktop: top bar, app launcher, dock, and Files / Terminal / Claude / Monitor / Settings windows |

### Claude integration (switchable)

| Backend | How it connects | Key stored? |
|---|---|---|
| **cli** (default) | Pipes the conversation to your signed-in Claude Code CLI (`claude -p`) | No |
| **api** | Streams from the Anthropic Messages API | Yes (`ANTHROPIC_API_KEY` or Settings) |
| **auto** | API if a key exists, otherwise CLI | – |

Switch in **Settings**, or run `backend cli|api|auto` in the shell.

Claude knows which OS and folder you're in. When it suggests a command, the
command appears with a **Run** button (desktop) or a `Run it? [y/N]` prompt
(shell). **Nothing runs until you confirm.**

## Install

1. Install Rust: <https://rustup.rs>
2. For the CLI backend, install Claude Code and sign in once:
   ```
   npm install -g @anthropic-ai/claude-code
   claude
   ```
3. Build:
   ```
   cargo build --release
   ```
   The binaries are in `target/release/`: `orbit` and `orbit-desktop`
   (`.exe` on Windows).

### Platform notes
- **Linux**: needs X11 or Wayland, plus OpenGL (Mesa). On Debian/Ubuntu, if the
  build fails: `sudo apt install libxkbcommon-dev libgl1-mesa-dev`
- **macOS**: works out of the box. Run `cargo build --release` in Terminal.
- **Windows**: install the "Desktop development with C++" workload from the
  Visual Studio Build Tools (Rust's MSVC toolchain needs it).

## Use

### Desktop
```
orbit-desktop              # windowed
orbit-desktop --fullscreen # takes over the screen, like an OS
orbit-desktop --show-all   # open every window at start
```
- **Ctrl/Cmd + Space**: app launcher (searches your installed apps on every OS)
- **Ctrl/Cmd + K**: Claude
- In Files, right-click a file and choose **Ask Claude about this file**
- In Terminal, type `? question` to send it to Claude
- **Send system info** sends live CPU, memory and disk stats to Claude for a health check

### Shell
```
orbit                      # interactive
orbit ? what's using my RAM
```
| Command | Does |
|---|---|
| `? <question>` | Chat with Claude (it remembers the conversation) |
| `do <task>` | Claude writes the command; you confirm |
| `explain <file>` | Claude explains a file |
| `ls cd pwd cat open` | File basics |
| `sys ps kill apps launch` | System and apps |
| `backend model key config` | Settings |
| anything else | Runs in cmd.exe / sh |

## Make it your session on Linux (boot into OrbitOS)
```
sudo cp target/release/orbit-desktop /usr/local/bin/
sudo cp packaging/orbitos.desktop /usr/share/xsessions/
```
Log out, pick **OrbitOS** on the login screen, and it starts fullscreen.

## Settings file
- Linux: `~/.config/orbitos/config.toml`
- macOS: `~/Library/Application Support/orbitos/config.toml`
- Windows: `%APPDATA%\orbitos\config.toml`

```toml
backend = "cli"            # cli | api | auto
model = "claude-sonnet-5-5"
max_tokens = 4096
fullscreen = false
# api_key = "..."          # optional; ANTHROPIC_API_KEY takes priority
# cli_path = "/path/to/claude"
```
A saved API key is stored in plain text. An environment variable is safer.

## Develop
```
cargo test --workspace
cargo run -p orbit-desktop
cargo run -p orbit-shell
```
GitHub Actions (`.github/workflows/ci.yml`) builds and tests on Windows,
macOS and Linux.

## Roadmap ideas
- Claude tool use: file edits and app control with per-action approval
- Window manager mode on Linux (Wayland compositor via `smithay`)
- Notifications, clipboard history, a settings sync
- Voice input
- Plugins (WASM) for third-party widgets

MIT licensed.
