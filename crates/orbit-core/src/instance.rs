//! Single instance: only one Orbit runs per user. A second launch hands its
//! request ("show", "toggle", "hide", "quit") to the running copy over a
//! local socket and exits.
//!
//! The socket listens on 127.0.0.1 only. Every message must start with a
//! random token kept in this user's data folder, so other users and other
//! machines cannot drive your Orbit.

use anyhow::{bail, Context, Result};
use rand::RngCore;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::time::Duration;

/// Fixed local port for Orbit's single-instance channel.
pub const PORT: u16 = 47_317;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Show,
    Hide,
    Toggle,
    Search,
    Quit,
}

impl Command {
    pub fn as_str(self) -> &'static str {
        match self {
            Command::Show => "show",
            Command::Hide => "hide",
            Command::Toggle => "toggle",
            Command::Search => "search",
            Command::Quit => "quit",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "show" => Some(Command::Show),
            "hide" => Some(Command::Hide),
            "toggle" => Some(Command::Toggle),
            "search" => Some(Command::Search),
            "quit" => Some(Command::Quit),
            _ => None,
        }
    }

    /// The command a command-line argument asks for, if any.
    pub fn from_args(args: &[String]) -> Option<Self> {
        args.iter().find_map(|a| match a.as_str() {
            "--show" => Some(Command::Show),
            "--hide" => Some(Command::Hide),
            "--toggle" => Some(Command::Toggle),
            "--search" => Some(Command::Search),
            "--quit" => Some(Command::Quit),
            _ => None,
        })
    }
}

fn token_path() -> PathBuf {
    crate::Config::data_dir().join("instance.token")
}

fn addr(port: u16) -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], port))
}

/// The running copy's end: accepts commands from later launches.
pub struct Primary {
    listener: TcpListener,
    token: String,
}

/// Become the running copy, or hand `cmd` to the one already running.
pub enum Start {
    /// We are the only Orbit: keep this to receive commands.
    Primary(Primary),
    /// Another Orbit is running and has been sent the command.
    Forwarded,
}

pub fn start(cmd: Command) -> Result<Start> {
    start_on(PORT, cmd)
}

pub fn start_on(port: u16, cmd: Command) -> Result<Start> {
    match TcpListener::bind(addr(port)) {
        Ok(listener) => {
            listener.set_nonblocking(true)?;
            let mut bytes = [0u8; 16];
            rand::thread_rng().fill_bytes(&mut bytes);
            let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
            std::fs::write(token_path_for(port), &token)?;
            Ok(Start::Primary(Primary { listener, token }))
        }
        Err(_) => {
            send_on(port, cmd)
                .context("another program is using Orbit's port, and it isn't Orbit")?;
            Ok(Start::Forwarded)
        }
    }
}

fn token_path_for(port: u16) -> PathBuf {
    if port == PORT {
        token_path()
    } else {
        crate::Config::data_dir().join(format!("instance-{port}.token"))
    }
}

/// Send a command to the running Orbit.
pub fn send(cmd: Command) -> Result<()> {
    send_on(PORT, cmd)
}

fn send_on(port: u16, cmd: Command) -> Result<()> {
    let token = std::fs::read_to_string(token_path_for(port)).context("no running Orbit found")?;
    let mut s = TcpStream::connect_timeout(&addr(port), Duration::from_millis(800))?;
    s.set_read_timeout(Some(Duration::from_secs(2)))?;
    writeln!(s, "{} {}", token.trim(), cmd.as_str())?;
    let mut reply = String::new();
    BufReader::new(s).read_line(&mut reply)?;
    if reply.trim() != "ok" {
        bail!("the running Orbit refused the request");
    }
    Ok(())
}

impl Primary {
    /// Commands waiting from other launches (never blocks).
    pub fn poll(&self) -> Vec<Command> {
        let mut out = Vec::new();
        while let Ok((stream, peer)) = self.listener.accept() {
            if !peer.ip().is_loopback() {
                continue;
            }
            let _ = stream.set_nonblocking(false);
            let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
            let mut line = String::new();
            let mut reader = BufReader::new(match stream.try_clone() {
                Ok(s) => s,
                Err(_) => continue,
            });
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            let mut parts = line.trim().splitn(2, ' ');
            let (tok, cmd) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
            let mut w = stream;
            match (tok == self.token, Command::parse(cmd)) {
                (true, Some(c)) => {
                    let _ = writeln!(w, "ok");
                    out.push(c);
                }
                _ => {
                    let _ = writeln!(w, "denied");
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_launch_forwards_to_first() {
        let port = 47_900 + (std::process::id() % 50) as u16;
        let primary = match start_on(port, Command::Show).unwrap() {
            Start::Primary(p) => p,
            Start::Forwarded => panic!("port already taken"),
        };
        // The second launch waits for the first to answer, as it would in the app.
        let second = std::thread::spawn(move || {
            matches!(start_on(port, Command::Toggle).unwrap(), Start::Forwarded)
        });
        let mut got = Vec::new();
        for _ in 0..40 {
            got.extend(primary.poll());
            if !got.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(second.join().unwrap());
        assert_eq!(got, vec![Command::Toggle]);
        // A wrong token is refused.
        let mut s = TcpStream::connect(addr(port)).unwrap();
        writeln!(s, "nope quit").unwrap();
        let mut reply = String::new();
        let reader_thread = std::thread::spawn(move || {
            BufReader::new(s).read_line(&mut reply).unwrap();
            reply
        });
        std::thread::sleep(Duration::from_millis(100));
        assert!(primary.poll().is_empty());
        assert_eq!(reader_thread.join().unwrap().trim(), "denied");
        let _ = std::fs::remove_file(token_path_for(port));
    }

    #[test]
    fn args() {
        assert_eq!(
            Command::from_args(&["x".into(), "--toggle".into()]),
            Some(Command::Toggle)
        );
        assert_eq!(Command::from_args(&["--background".into()]), None);
    }
}
