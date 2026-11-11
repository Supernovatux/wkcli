use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum QuestionOrder {
    #[default]
    ReadingFirst,
    MeaningFirst,
    Random,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ReviewSettings {
    /// Ask both questions of an item one after the other.
    pub back_to_back: bool,
    /// Which question comes first for items that have both.
    pub question_order: QuestionOrder,
    /// Skip straight to the next question after a correct answer.
    pub lightning: bool,
    /// Double-check: Backspace after grading discards the answer so it can be retyped.
    pub retype: bool,
    /// Double-check: shake instead of marking wrong on likely slips.
    pub shake_near_miss: bool,
    /// Double-check: block advancing for this long after a wrong answer (0 = off).
    pub wrong_delay_ms: u64,
    /// Play the pronunciation (random voice) after a vocabulary reading is answered.
    pub audio: bool,
}

impl Default for ReviewSettings {
    fn default() -> Self {
        Self {
            back_to_back: false,
            question_order: QuestionOrder::default(),
            lightning: false,
            retype: true,
            shake_near_miss: true,
            wrong_delay_ms: 1500,
            audio: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub api_token: Option<String>,
    /// auto | kitty | sixel | iterm2 | halfblocks
    pub image_protocol: String,
    /// Draw kanji and vocabulary as large images (needs a graphics-capable terminal).
    pub big_text: bool,
    /// Font family for big text.
    pub font: String,
    pub review: ReviewSettings,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_token: None,
            image_protocol: "auto".into(),
            big_text: true,
            font: "Noto Sans CJK JP".into(),
            review: ReviewSettings::default(),
        }
    }
}

pub fn config_path() -> Result<PathBuf> {
    Ok(dirs::config_dir()
        .context("cannot determine config directory")?
        .join("wkcli")
        .join("config.toml"))
}

pub fn cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("wkcli"))
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        let cfg: Config = match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text)
                .with_context(|| format!("invalid config file {}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        Ok(cfg)
    }

    pub fn save(&self) -> Result<()> {
        let path = config_path()?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = toml::to_string_pretty(self)?;
        std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    /// Token from the environment (preferred) or the config file.
    pub fn token(&self) -> Option<String> {
        std::env::var("WANIKANI_API_TOKEN")
            .ok()
            .filter(|t| !t.trim().is_empty())
            .or_else(|| self.api_token.clone())
            .map(|t| t.trim().to_string())
    }
}
