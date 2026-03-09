//! Configuration for Hibiki — shikumi-based discovery, env overrides, hot-reload.
//!
//! Config file: `~/.config/hibiki/hibiki.yaml`
//! Env override: `HIBIKI_CONFIG=/path/to/config.yaml`
//! Field overrides: `HIBIKI_MUSIC_DIR=~/Music`, `HIBIKI_AUDIO__SAMPLE_RATE=48000`

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Top-level hibiki configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HibikiConfig {
    /// Root music directory to scan on startup.
    #[serde(default = "default_music_dir")]
    pub music_dir: PathBuf,
    /// Audio playback settings.
    #[serde(default)]
    pub audio: AudioConfig,
    /// BitTorrent client settings.
    #[serde(default)]
    pub torrent: TorrentConfig,
    /// Visual appearance settings.
    #[serde(default)]
    pub appearance: AppearanceConfig,
    /// Library management settings.
    #[serde(default)]
    pub library: LibraryConfig,
}

/// Audio playback configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    /// Target sample rate for output.
    #[serde(default = "default_sample_rate")]
    pub sample_rate: u32,
    /// Audio buffer size in frames.
    #[serde(default = "default_buffer_size")]
    pub buffer_size: u32,
    /// Enable gapless playback between tracks.
    #[serde(default = "default_gapless")]
    pub gapless: bool,
    /// Output device name (null = system default).
    #[serde(default)]
    pub output_device: Option<String>,
}

/// BitTorrent client configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentConfig {
    /// Directory for torrent downloads.
    #[serde(default = "default_download_dir")]
    pub download_dir: PathBuf,
    /// Maximum number of peer connections.
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
    /// Enable DHT for peer discovery.
    #[serde(default = "default_dht")]
    pub dht_enabled: bool,
    /// Maximum upload speed in KiB/s (0 = unlimited).
    #[serde(default)]
    pub max_upload_kbps: u32,
    /// Seed ratio limit (stop seeding after this ratio).
    #[serde(default = "default_seed_ratio")]
    pub seed_ratio_limit: f32,
    /// Automatically import completed downloads into the library.
    #[serde(default = "default_auto_import")]
    pub auto_import: bool,
}

/// Visual appearance configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppearanceConfig {
    /// Background color (hex).
    #[serde(default = "default_bg")]
    pub background: String,
    /// Foreground/text color (hex).
    #[serde(default = "default_fg")]
    pub foreground: String,
    /// Accent color for highlights (hex).
    #[serde(default = "default_accent")]
    pub accent: String,
    /// Visualizer mode.
    #[serde(default = "default_visualizer")]
    pub visualizer: String,
}

/// Library management configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryConfig {
    /// Scan music directories on startup.
    #[serde(default = "default_scan_on_startup")]
    pub scan_on_startup: bool,
    /// Watch directories for changes (FSEvents/inotify).
    #[serde(default)]
    pub watch_dirs: bool,
    /// Metadata source: "tags", "filename", or "both".
    #[serde(default = "default_metadata_source")]
    pub metadata_source: String,
}

// --- Default implementations ---

impl Default for HibikiConfig {
    fn default() -> Self {
        Self {
            music_dir: default_music_dir(),
            audio: AudioConfig::default(),
            torrent: TorrentConfig::default(),
            appearance: AppearanceConfig::default(),
            library: LibraryConfig::default(),
        }
    }
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            sample_rate: default_sample_rate(),
            buffer_size: default_buffer_size(),
            gapless: default_gapless(),
            output_device: None,
        }
    }
}

impl Default for TorrentConfig {
    fn default() -> Self {
        Self {
            download_dir: default_download_dir(),
            max_connections: default_max_connections(),
            dht_enabled: default_dht(),
            max_upload_kbps: 0,
            seed_ratio_limit: default_seed_ratio(),
            auto_import: default_auto_import(),
        }
    }
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            background: default_bg(),
            foreground: default_fg(),
            accent: default_accent(),
            visualizer: default_visualizer(),
        }
    }
}

impl Default for LibraryConfig {
    fn default() -> Self {
        Self {
            scan_on_startup: default_scan_on_startup(),
            watch_dirs: false,
            metadata_source: default_metadata_source(),
        }
    }
}

// --- Default value functions ---

fn default_music_dir() -> PathBuf {
    dirs::audio_dir().unwrap_or_else(|| dirs::home_dir().unwrap().join("Music"))
}
fn default_sample_rate() -> u32 {
    44100
}
fn default_buffer_size() -> u32 {
    4096
}
fn default_gapless() -> bool {
    true
}
fn default_download_dir() -> PathBuf {
    default_music_dir().join("Downloads")
}
fn default_max_connections() -> u32 {
    50
}
fn default_dht() -> bool {
    true
}
fn default_seed_ratio() -> f32 {
    2.0
}
fn default_auto_import() -> bool {
    true
}
fn default_bg() -> String {
    "#2e3440".into()
}
fn default_fg() -> String {
    "#eceff4".into()
}
fn default_accent() -> String {
    "#88c0d0".into()
}
fn default_visualizer() -> String {
    "spectrum".into()
}
fn default_scan_on_startup() -> bool {
    true
}
fn default_metadata_source() -> String {
    "tags".into()
}

/// Load configuration from disk via shikumi, with optional path override.
///
/// Falls back to defaults if no config file is found.
///
/// # Errors
///
/// Returns an error if the config file exists but cannot be parsed.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_reasonable_values() {
        let config = HibikiConfig::default();
        assert_eq!(config.audio.sample_rate, 44100);
        assert_eq!(config.audio.buffer_size, 4096);
        assert!(config.audio.gapless);
        assert!(config.audio.output_device.is_none());
        assert_eq!(config.torrent.max_connections, 50);
        assert!(config.torrent.dht_enabled);
        assert!(config.torrent.auto_import);
        assert!((config.torrent.seed_ratio_limit - 2.0).abs() < f32::EPSILON);
        assert_eq!(config.appearance.background, "#2e3440");
        assert_eq!(config.appearance.visualizer, "spectrum");
        assert!(config.library.scan_on_startup);
        assert_eq!(config.library.metadata_source, "tags");
    }

    #[test]
    fn serde_roundtrip() {
        let config = HibikiConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: HibikiConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.audio.sample_rate, config.audio.sample_rate);
        assert_eq!(parsed.appearance.accent, config.appearance.accent);
    }
}
