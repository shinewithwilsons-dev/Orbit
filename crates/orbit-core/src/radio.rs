//! Internet radio (radio-browser.info, a free open directory of 50,000+
//! stations) and BeMusic streaming, played through mpv or VLC.

use anyhow::{bail, Context, Result};
use std::process::{Child, Command, Stdio};

#[derive(Debug, Clone)]
pub struct Station {
    pub name: String,
    pub url: String,
    pub info: String,
}

/// Hand-picked stations, including SomaFM's space channels.
pub fn featured() -> Vec<Station> {
    let s = |name: &str, url: &str, info: &str| Station {
        name: name.into(),
        url: url.into(),
        info: info.into(),
    };
    vec![
        s(
            "SomaFM Deep Space One",
            "https://ice1.somafm.com/deepspaceone-128-mp3",
            "Deep ambient for space travel",
        ),
        s(
            "SomaFM Space Station Soma",
            "https://ice1.somafm.com/spacestation-128-mp3",
            "Mid-tempo electronica",
        ),
        s(
            "SomaFM Mission Control",
            "https://ice1.somafm.com/missioncontrol-128-mp3",
            "Ambient with real NASA mission audio",
        ),
        s(
            "SomaFM Drone Zone",
            "https://ice1.somafm.com/dronezone-128-mp3",
            "Atmospheric ambient",
        ),
        s(
            "SomaFM Groove Salad",
            "https://ice1.somafm.com/groovesalad-128-mp3",
            "Downtempo and chill",
        ),
    ]
}

/// Search radio-browser.info by name or genre.
pub fn search(query: &str) -> Result<Vec<Station>> {
    let url = format!(
        "https://all.api.radio-browser.info/json/stations/search?name={}&limit=30&hidebroken=true&order=clickcount&reverse=true",
        crate::url_encode(query)
    );
    let v: serde_json::Value = ureq::get(&url)
        .set("User-Agent", concat!("OrbitOS/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(10))
        .call()
        .context("radio directory unavailable")?
        .into_json()?;
    Ok(v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|st| {
                    let url = st["url_resolved"]
                        .as_str()
                        .or_else(|| st["url"].as_str())?
                        .to_string();
                    Some(Station {
                        name: st["name"].as_str()?.trim().to_string(),
                        url,
                        info: format!(
                            "{} · {} · {} kbps",
                            st["countrycode"].as_str().unwrap_or(""),
                            st["tags"]
                                .as_str()
                                .unwrap_or("")
                                .split(',')
                                .take(3)
                                .collect::<Vec<_>>()
                                .join(", "),
                            st["bitrate"].as_i64().unwrap_or(0)
                        ),
                    })
                })
                .collect()
        })
        .unwrap_or_default())
}

/// A background audio stream (radio or BeMusic) played by mpv or VLC.
#[derive(Default)]
pub struct StreamPlayer {
    child: Option<Child>,
    pub title: String,
}

impl StreamPlayer {
    pub fn play(&mut self, url: &str, title: &str, headers: Option<&str>) -> Result<()> {
        self.stop();
        let child = if let Some(mpv) = crate::media::find_mpv() {
            let mut c = Command::new(mpv);
            c.args(["--no-video", "--really-quiet", "--no-terminal"]);
            if let Some(h) = headers {
                c.arg(format!("--http-header-fields={h}"));
            }
            c.arg(url)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?
        } else if let Some(vlc) = crate::media::find_vlc() {
            Command::new(vlc)
                .args(["-I", "dummy", "--no-video", url])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?
        } else {
            bail!(
                "streaming needs mpv or VLC: {}",
                crate::media::player_install_hint()
            );
        };
        self.child = Some(child);
        self.title = title.to_string();
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        self.title.clear();
    }

    pub fn is_playing(&mut self) -> bool {
        match self.child.as_mut().map(|c| c.try_wait()) {
            Some(Ok(None)) => true,
            Some(_) => {
                self.child = None;
                false
            }
            None => false,
        }
    }
}

impl Drop for StreamPlayer {
    fn drop(&mut self) {
        self.stop();
    }
}

// ------------------------------------------------------------ BeMusic ----

#[derive(Debug, Clone)]
pub struct RemoteTrack {
    pub title: String,
    pub artist: String,
    pub url: String,
}

/// Search your BeMusic site (`/api/v1/search`). Needs the site URL and an
/// API token from your BeMusic account settings.
pub fn bemusic_search(cfg: &crate::Config, query: &str) -> Result<Vec<RemoteTrack>> {
    let base = cfg
        .bemusic_url
        .as_deref()
        .filter(|u| !u.trim().is_empty())
        .context("add your BeMusic site URL in Settings -> Media")?;
    let base = base.trim_end_matches('/');
    let mut req = ureq::get(&format!(
        "{base}/api/v1/search?query={}&limit=25",
        crate::url_encode(query)
    ))
    .set("Accept", "application/json")
    .timeout(std::time::Duration::from_secs(15));
    if let Some(t) = cfg.bemusic_token.as_ref().filter(|t| !t.is_empty()) {
        req = req.set("Authorization", &format!("Bearer {t}"));
    }
    let v: serde_json::Value = match req.call() {
        Ok(r) => r.into_json()?,
        Err(ureq::Error::Status(code, _)) => {
            bail!("BeMusic returned {code} (check the URL and API token)")
        }
        Err(e) => bail!("can't reach BeMusic: {e}"),
    };
    Ok(parse_bemusic(&v, base))
}

/// Tolerant parser: BeMusic versions nest results slightly differently.
pub fn parse_bemusic(v: &serde_json::Value, base: &str) -> Vec<RemoteTrack> {
    let candidates = [
        &v["results"]["tracks"]["data"],
        &v["results"]["tracks"],
        &v["tracks"]["data"],
        &v["tracks"],
    ];
    let Some(list) = candidates.iter().find_map(|c| c.as_array()) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|t| {
            let title = t["name"].as_str()?.to_string();
            let artist = t["artists"][0]["name"].as_str().unwrap_or("").to_string();
            let src = t["src"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .or_else(|| {
                    t["id"]
                        .as_i64()
                        .map(|id| format!("{base}/api/v1/tracks/{id}/download"))
                })?;
            let url = if src.starts_with("http") {
                src
            } else {
                format!("{base}/{}", src.trim_start_matches('/'))
            };
            Some(RemoteTrack { title, artist, url })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bemusic_shapes() {
        let v = serde_json::json!({"results": {"tracks": [
            {"id": 7, "name": "Song", "artists": [{"name": "Band"}], "src": "storage/track.mp3"},
            {"id": 8, "name": "Other", "artists": []}
        ]}});
        let t = parse_bemusic(&v, "https://music.example");
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].url, "https://music.example/storage/track.mp3");
        assert_eq!(t[1].url, "https://music.example/api/v1/tracks/8/download");
        assert_eq!(t[0].artist, "Band");
    }

    #[test]
    fn featured_list() {
        assert!(featured().iter().all(|s| s.url.starts_with("https://")));
    }
}
