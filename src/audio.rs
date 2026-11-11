//! Vocabulary pronunciation audio: downloaded and played on a background thread so the
//! UI never waits. Files are cached on disk. All failures are silent (no sound).

use crate::config;
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

enum Cmd {
    /// Download ahead of time so playback starts instantly.
    Prefetch(String),
    Play(String),
}

pub struct Audio {
    tx: Sender<Cmd>,
}

pub fn cache_path(url: &str) -> PathBuf {
    let name: String = url
        .rsplit('/')
        .next()
        .unwrap_or("audio")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .collect();
    config::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("audio")
        .join(if name.is_empty() { "audio".to_string() } else { name })
}

fn fetch(agent: &ureq::Agent, url: &str) -> Option<PathBuf> {
    let path = cache_path(url);
    if path.exists() {
        return Some(path);
    }
    let mut resp = agent.get(url).call().ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let bytes = resp.body_mut().read_to_vec().ok()?;
    std::fs::create_dir_all(path.parent()?).ok()?;
    std::fs::write(&path, bytes).ok()?;
    Some(path)
}

fn decode(path: &PathBuf) -> Option<rodio::Decoder<std::io::BufReader<std::fs::File>>> {
    let file = std::fs::File::open(path).ok()?;
    rodio::Decoder::try_from(file).ok()
}

impl Audio {
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel::<Cmd>();
        std::thread::spawn(move || {
            let config = ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(15)))
                .http_status_as_error(false)
                .build();
            let agent = ureq::Agent::new_with_config(config);
            // The output device is opened on first use and kept for the session.
            let mut sink: Option<rodio::MixerDeviceSink> = None;
            let mut device_failed = false;
            let mut player: Option<rodio::Player> = None;
            for cmd in rx {
                match cmd {
                    Cmd::Prefetch(url) => {
                        fetch(&agent, &url);
                    }
                    Cmd::Play(url) => {
                        let Some(path) = fetch(&agent, &url) else { continue };
                        if sink.is_none() && !device_failed {
                            match rodio::DeviceSinkBuilder::open_default_sink() {
                                Ok(mut s) => {
                                    s.log_on_drop(false);
                                    sink = Some(s);
                                }
                                Err(_) => device_failed = true,
                            }
                        }
                        let (Some(sink), Some(source)) = (&sink, decode(&path)) else { continue };
                        // Replacing the player cuts off whatever was still playing.
                        let p = rodio::Player::connect_new(sink.mixer());
                        p.append(source);
                        player = Some(p);
                    }
                }
            }
            drop(player);
        });
        Self { tx }
    }

    pub fn prefetch(&self, url: &str) {
        let _ = self.tx.send(Cmd::Prefetch(url.to_string()));
    }

    pub fn play(&self, url: &str) {
        let _ = self.tx.send(Cmd::Play(url.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Manual check: WKCLI_TEST_MP3=/path/to.mp3 cargo test -- --ignored plays_file
    #[test]
    #[ignore]
    fn plays_file() {
        let path = PathBuf::from(std::env::var("WKCLI_TEST_MP3").expect("set WKCLI_TEST_MP3"));
        let source = decode(&path).expect("decodable mp3");
        let sink = rodio::DeviceSinkBuilder::open_default_sink().expect("audio device");
        let player = rodio::Player::connect_new(sink.mixer());
        player.append(source);
        player.sleep_until_end();
    }
}
