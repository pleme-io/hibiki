use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HibikiConfig {
    #[serde(default = "default_music_dir")]
    pub music_dir: PathBuf,
    #[serde(default)]
    pub audio: AudioConfig,
    #[serde(default)]
    pub torrent: TorrentConfig,
    #[serde(default)]
    pub appearance: AppearanceConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    #[serde(default = "default_sample_rate")]
    pub sample_rate: u32,
    #[serde(default = "default_buffer_size")]
    pub buffer_size: u32,
    #[serde(default)]
    pub gapless: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentConfig {
    #[serde(default = "default_download_dir")]
    pub download_dir: PathBuf,
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
    #[serde(default)]
    pub dht_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppearanceConfig {
    #[serde(default = "default_bg")]
    pub background: String,
    #[serde(default = "default_fg")]
    pub foreground: String,
    #[serde(default = "default_accent")]
    pub accent: String,
}

impl Default for HibikiConfig {
    fn default() -> Self {
        Self {
            music_dir: default_music_dir(),
            audio: AudioConfig::default(),
            torrent: TorrentConfig::default(),
            appearance: AppearanceConfig::default(),
        }
    }
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self { sample_rate: default_sample_rate(), buffer_size: default_buffer_size(), gapless: true }
    }
}

impl Default for TorrentConfig {
    fn default() -> Self {
        Self {
            download_dir: default_download_dir(),
            max_connections: default_max_connections(),
            dht_enabled: true,
        }
    }
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self { background: default_bg(), foreground: default_fg(), accent: default_accent() }
    }
}

fn default_music_dir() -> PathBuf { dirs::audio_dir().unwrap_or_else(|| dirs::home_dir().unwrap().join("Music")) }
fn default_sample_rate() -> u32 { 44100 }
fn default_buffer_size() -> u32 { 4096 }
fn default_download_dir() -> PathBuf { default_music_dir().join("Downloads") }
fn default_max_connections() -> u32 { 50 }
fn default_bg() -> String { "#2e3440".into() }
fn default_fg() -> String { "#eceff4".into() }
fn default_accent() -> String { "#88c0d0".into() }

pub fn load(override_path: &Option<PathBuf>) -> anyhow::Result<HibikiConfig> {
    let path = match override_path {
        Some(p) => p.clone(),
        None => match shikumi::ConfigDiscovery::new("hibiki")
            .env_override("HIBIKI_CONFIG")
            .discover()
        {
            Ok(p) => p,
            Err(_) => {
                tracing::info!("no config file found, using defaults");
                return Ok(HibikiConfig::default());
            }
        },
    };

    let store = shikumi::ConfigStore::<HibikiConfig>::load(&path, "HIBIKI_")?;
    Ok(HibikiConfig::clone(&store.get()))
}
