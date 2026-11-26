//! Backup and sync: settings, Claude chats, playlists, quarantine records,
//! window layout and Shield signatures in one file. With a password the
//! file is encrypted (Argon2 key + AES-256-GCM). Point the backup folder at
//! OneDrive, iCloud Drive, Dropbox or Google Drive to sync between machines.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{bail, Context, Result};
use rand::RngCore;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 5] = b"ORBK1";
const SKIP_EXT: &[&str] = &["bin", "gguf", "qbin"];

fn config_dir() -> PathBuf {
    crate::Config::path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| ".".into())
}

fn collect(root: &Path, prefix: &str, out: &mut Vec<(String, PathBuf)>) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if !p
                .extension()
                .map(|x| SKIP_EXT.contains(&x.to_string_lossy().as_ref()))
                .unwrap_or(false)
            {
                if let Ok(rel) = p.strip_prefix(root) {
                    let name = format!("{prefix}/{}", rel.to_string_lossy().replace('\\', "/"));
                    out.push((name, p));
                }
            }
        }
    }
}

/// Zip everything up. `strip_secrets` blanks API keys (used for automatic
/// unencrypted backups).
fn build_zip(strip_secrets: bool) -> Result<Vec<u8>> {
    let mut files = Vec::new();
    collect(&config_dir(), "config", &mut files);
    collect(&crate::Config::data_dir(), "data", &mut files);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, path) in files {
        // A file can vanish between listing and reading (the app or another
        // process is still using the data folder). Skip it rather than fail.
        let mut data = match std::fs::read(&path) {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };
        if strip_secrets && name == "config/config.toml" {
            let mut cfg: crate::Config =
                toml::from_str(&String::from_utf8_lossy(&data)).unwrap_or_default();
            cfg.api_key = None;
            cfg.local_api_key = None;
            cfg.bemusic_token = None;
            data = toml::to_string_pretty(&cfg)?.into_bytes();
        }
        zip.start_file(name, opts)?;
        zip.write_all(&data)?;
    }
    Ok(zip.finish()?.into_inner())
}

fn key(password: &str, salt: &[u8]) -> Result<[u8; 32]> {
    let mut k = [0u8; 32];
    argon2::Argon2::default()
        .hash_password_into(password.as_bytes(), salt, &mut k)
        .map_err(|e| anyhow::anyhow!("key derivation failed: {e}"))?;
    Ok(k)
}

/// Write a backup into `dir`. Empty password = unencrypted, secrets removed.
pub fn create(dir: &Path, password: &str) -> Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let encrypted = !password.is_empty();
    let zip = build_zip(!encrypted)?;
    let mut out = Vec::with_capacity(zip.len() + 64);
    out.extend_from_slice(MAGIC);
    if encrypted {
        let mut salt = [0u8; 16];
        let mut nonce = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut salt);
        rand::thread_rng().fill_bytes(&mut nonce);
        let cipher = Aes256Gcm::new_from_slice(&key(password, &salt)?)?;
        let ct = cipher
            .encrypt(Nonce::from_slice(&nonce), zip.as_ref())
            .map_err(|_| anyhow::anyhow!("encryption failed"))?;
        out.push(1);
        out.extend_from_slice(&salt);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ct);
    } else {
        out.push(0);
        out.extend_from_slice(&zip);
    }
    let stamp = chrono::Local::now().format("%Y-%m-%d_%H%M%S%.3f");
    let path = dir.join(format!("OrbitOS-backup-{stamp}.orbitbackup"));
    std::fs::write(&path, out)?;
    Ok(path)
}

/// Restore a backup over the current settings and data.
pub fn restore(file: &Path, password: &str) -> Result<usize> {
    let raw = std::fs::read(file)?;
    if raw.len() < 6 || &raw[..5] != MAGIC {
        bail!("not an OrbitOS backup");
    }
    let zip = match raw[5] {
        0 => raw[6..].to_vec(),
        1 => {
            if raw.len() < 6 + 28 {
                bail!("backup is damaged");
            }
            if password.is_empty() {
                bail!("this backup is encrypted: enter its password");
            }
            let (salt, rest) = raw[6..].split_at(16);
            let (nonce, ct) = rest.split_at(12);
            let cipher = Aes256Gcm::new_from_slice(&key(password, salt)?)?;
            cipher
                .decrypt(Nonce::from_slice(nonce), ct)
                .map_err(|_| anyhow::anyhow!("wrong password or damaged backup"))?
        }
        _ => bail!("unknown backup format"),
    };
    let mut archive = zip::ZipArchive::new(Cursor::new(zip)).context("backup is damaged")?;
    let (cfg_dir, data_dir) = (config_dir(), crate::Config::data_dir());
    let mut count = 0;
    for i in 0..archive.len() {
        let mut f = archive.by_index(i)?;
        let name = f.name().to_string();
        if name.contains("..") {
            continue; // never write outside our folders
        }
        let target = if let Some(rest) = name.strip_prefix("config/") {
            cfg_dir.join(rest)
        } else if let Some(rest) = name.strip_prefix("data/") {
            data_dir.join(rest)
        } else {
            continue;
        };
        if let Some(p) = target.parent() {
            std::fs::create_dir_all(p)?;
        }
        let mut data = Vec::new();
        f.read_to_end(&mut data)?;
        std::fs::write(target, data)?;
        count += 1;
    }
    Ok(count)
}

/// Backups in a folder, newest first.
pub fn list(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().map(|e| e == "orbitbackup").unwrap_or(false))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v.reverse();
    v
}

/// Suggested backup folders: cloud-synced folders that exist on this machine.
pub fn suggested_dirs() -> Vec<(String, PathBuf)> {
    let Some(h) = dirs::home_dir() else {
        return Vec::new();
    };
    let mut v = Vec::new();
    for (name, rel) in [
        ("OneDrive", "OneDrive"),
        (
            "iCloud Drive",
            "Library/Mobile Documents/com~apple~CloudDocs",
        ),
        ("Dropbox", "Dropbox"),
        ("Google Drive", "Google Drive"),
    ] {
        let p = h.join(rel);
        if p.exists() {
            v.push((name.to_string(), p.join("OrbitOS Backups")));
        }
    }
    v.push((
        "Documents".into(),
        crate::user_dirs::documents()
            .unwrap_or(h)
            .join("OrbitOS Backups"),
    ));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_roundtrip() {
        let salt = [7u8; 16];
        let k = key("pw", &salt).unwrap();
        let cipher = Aes256Gcm::new_from_slice(&k).unwrap();
        let n = [1u8; 12];
        let ct = cipher
            .encrypt(Nonce::from_slice(&n), b"hello".as_ref())
            .unwrap();
        assert_ne!(&ct[..5], b"hello");
        let wrong = Aes256Gcm::new_from_slice(&key("nope", &salt).unwrap()).unwrap();
        assert!(wrong.decrypt(Nonce::from_slice(&n), ct.as_ref()).is_err());
        assert_eq!(
            cipher.decrypt(Nonce::from_slice(&n), ct.as_ref()).unwrap(),
            b"hello"
        );
    }

    #[test]
    fn backup_file_roundtrip() {
        let d = std::env::temp_dir().join(format!("orbit-bk-{}", std::process::id()));
        let f = create(&d, "secret").unwrap();
        assert!(restore(&f, "wrong").is_err());
        assert!(restore(&f, "secret").is_ok());
        let plain = create(&d, "").unwrap();
        assert!(restore(&plain, "").is_ok());
        assert_eq!(list(&d).len(), 2);
        std::fs::remove_dir_all(d).ok();
    }
}
