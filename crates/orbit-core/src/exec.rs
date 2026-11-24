//! Running commands and opening things on the host OS.

use anyhow::Result;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};

pub struct CmdOutput {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

fn shell_command(cmd: &str) -> Command {
    if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", cmd]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-c", cmd]);
        c
    }
}

/// Run a command through the host shell and capture its output.
pub fn run_capture(cmd: &str, cwd: &Path) -> Result<CmdOutput> {
    let out = shell_command(cmd)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .output()?;
    Ok(CmdOutput {
        status: out.status,
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

/// Run a command attached to the current terminal.
pub fn run_interactive(cmd: &str, cwd: &Path) -> Result<ExitStatus> {
    Ok(shell_command(cmd).current_dir(cwd).status()?)
}

/// Open a file, folder or URL with the default handler.
pub fn open(target: &str) -> Result<()> {
    let mut c = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/C", "start", "", target]);
        c
    } else if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        c.arg(target);
        c
    } else {
        let mut c = Command::new("xdg-open");
        c.arg(target);
        c
    };
    c.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

/// Start a shell command detached from OrbitOS (used to launch apps).
pub fn spawn_detached(cmd: &str) -> Result<()> {
    shell_command(cmd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_echo() {
        let out = run_capture("echo orbit", Path::new(".")).unwrap();
        assert!(out.status.success());
        assert!(out.stdout.contains("orbit"));
    }
}
