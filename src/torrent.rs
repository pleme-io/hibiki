//! BitTorrent client -- download and share hi-fi music.
//!
//! Uses librqbit for the BitTorrent protocol implementation.
//! Features: magnet links, .torrent files, DHT, download management,
//! automatic library import on completion.

use crate::config::TorrentConfig;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(thiserror::Error, Debug)]
#[allow(dead_code)]
pub enum TorrentError {
    #[error("invalid magnet uri: {0}")]
    InvalidMagnet(String),
    #[error("torrent file not found: {0}")]
    FileNotFound(PathBuf),
    #[error("download failed: {0}")]
    Download(String),
    #[error("client error: {0}")]
    Client(String),
    #[error("torrent not found: {0}")]
    NotFound(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, TorrentError>;

/// Status of an individual torrent.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TorrentStatus {
    /// Unique identifier for this torrent.
    pub id: String,
    /// Display name of the torrent.
    pub name: String,
    /// Download progress from 0.0 to 1.0.
    pub progress: f32,
    /// Total size in bytes.
    pub total_bytes: u64,
    /// Downloaded bytes.
    pub downloaded_bytes: u64,
    /// Current download speed in bytes per second.
    pub download_speed: u64,
    /// Current upload speed in bytes per second.
    pub upload_speed: u64,
    /// Number of connected peers.
    pub peers: u32,
    /// Current state of the torrent.
    pub state: TorrentState,
    /// Time the torrent was added.
    pub added_at: String,
}

/// Possible states of a torrent.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum TorrentState {
    /// Waiting for metadata (magnet link resolving).
    Metadata,
    /// Actively downloading.
    Downloading,
    /// Download complete, seeding to peers.
    Seeding,
    /// Manually paused.
    Paused,
    /// Verifying downloaded data.
    Checking,
    /// An error occurred.
    Error(String),
}

impl std::fmt::Display for TorrentState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Metadata => write!(f, "Resolving"),
            Self::Downloading => write!(f, "Downloading"),
            Self::Seeding => write!(f, "Seeding"),
            Self::Paused => write!(f, "Paused"),
            Self::Checking => write!(f, "Checking"),
            Self::Error(e) => write!(f, "Error: {e}"),
        }
    }
}

/// Internal record for a managed torrent.
#[allow(dead_code)]
struct ManagedTorrent {
    id: String,
    name: String,
    state: TorrentState,
    output_dir: PathBuf,
    total_bytes: u64,
    downloaded_bytes: u64,
    download_speed: u64,
    upload_speed: u64,
    peers: u32,
    added_at: Instant,
}

/// BitTorrent client for downloading music.
///
/// Wraps librqbit and manages torrents in the configured download directory.
/// Completed downloads can be enumerated via [`completed_paths`] for automatic
/// import into the music library.
pub struct TorrentClient {
    config: TorrentConfig,
    torrents: Vec<ManagedTorrent>,
    next_id: u64,
    // TODO: librqbit::Session handle -- requires async initialization
    // session: Option<librqbit::Session>,
}

#[allow(dead_code)]
impl TorrentClient {
    /// Create a new torrent client with the given configuration.
    ///
    /// The download directory is created if it does not exist.
    #[must_use]
    pub fn new(config: &TorrentConfig) -> Self {
        tracing::info!(dir = %config.download_dir.display(), "initialising torrent client");

        if !config.download_dir.exists() {
            if let Err(e) = std::fs::create_dir_all(&config.download_dir) {
                tracing::warn!(
                    dir = %config.download_dir.display(),
                    error = %e,
                    "failed to create download directory"
                );
            }
        }

        Self {
            config: config.clone(),
            torrents: Vec::new(),
            next_id: 1,
        }
    }

    /// Add a torrent from a magnet URI.
    ///
    /// Returns a torrent ID that can be used to query status.
    pub async fn add_magnet(&mut self, uri: &str) -> Result<String> {
        if !uri.starts_with("magnet:") {
            return Err(TorrentError::InvalidMagnet(format!(
                "URI does not start with 'magnet:': {uri}"
            )));
        }

        tracing::info!(uri, "adding magnet link");

        // Extract name from magnet URI if present (dn= parameter).
        let name =
            extract_magnet_name(uri).unwrap_or_else(|| format!("torrent-{}", self.next_id));

        let id = format!("torrent-{}", self.next_id);
        self.next_id += 1;

        let output_dir = self.config.download_dir.join(&name);

        self.torrents.push(ManagedTorrent {
            id: id.clone(),
            name,
            state: TorrentState::Metadata,
            output_dir,
            total_bytes: 0,
            downloaded_bytes: 0,
            download_speed: 0,
            upload_speed: 0,
            peers: 0,
            added_at: Instant::now(),
        });

        // TODO: Create librqbit session and add magnet
        // Once metadata is resolved, transition to Downloading state.

        tracing::info!(id = %id, "torrent added");
        Ok(id)
    }

    /// Add a torrent from a `.torrent` file.
    ///
    /// Returns a torrent ID that can be used to query status.
    pub async fn add_torrent_file(&mut self, path: &Path) -> Result<String> {
        if !path.exists() {
            return Err(TorrentError::FileNotFound(path.to_path_buf()));
        }

        if path.extension().and_then(|e| e.to_str()) != Some("torrent") {
            return Err(TorrentError::Client(format!(
                "file does not have .torrent extension: {}",
                path.display()
            )));
        }

        tracing::info!(path = %path.display(), "adding torrent file");

        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let id = format!("torrent-{}", self.next_id);
        self.next_id += 1;

        let output_dir = self.config.download_dir.join(&name);

        self.torrents.push(ManagedTorrent {
            id: id.clone(),
            name,
            state: TorrentState::Downloading,
            output_dir,
            total_bytes: 0,
            downloaded_bytes: 0,
            download_speed: 0,
            upload_speed: 0,
            peers: 0,
            added_at: Instant::now(),
        });

        // TODO: Read .torrent file and add to librqbit session

        tracing::info!(id = %id, "torrent file added");
        Ok(id)
    }

    /// Pause a torrent by ID.
    pub fn pause(&mut self, id: &str) -> Result<()> {
        let torrent = self
            .torrents
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or_else(|| TorrentError::NotFound(id.to_string()))?;

        torrent.state = TorrentState::Paused;
        tracing::info!(id, "torrent paused");
        Ok(())
    }

    /// Resume a paused torrent by ID.
    pub fn resume(&mut self, id: &str) -> Result<()> {
        let torrent = self
            .torrents
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or_else(|| TorrentError::NotFound(id.to_string()))?;

        if torrent.state == TorrentState::Paused {
            torrent.state = TorrentState::Downloading;
            tracing::info!(id, "torrent resumed");
        }
        Ok(())
    }

    /// Remove a torrent by ID.
    pub fn remove(&mut self, id: &str) -> Result<()> {
        let pos = self
            .torrents
            .iter()
            .position(|t| t.id == id)
            .ok_or_else(|| TorrentError::NotFound(id.to_string()))?;

        self.torrents.remove(pos);
        tracing::info!(id, "torrent removed");
        Ok(())
    }

    /// List the status of all managed torrents.
    #[must_use]
    pub fn list_torrents(&self) -> Vec<TorrentStatus> {
        self.torrents
            .iter()
            .map(|t| {
                let progress = if t.total_bytes > 0 {
                    t.downloaded_bytes as f32 / t.total_bytes as f32
                } else {
                    match &t.state {
                        TorrentState::Seeding => 1.0,
                        _ => 0.0,
                    }
                };

                TorrentStatus {
                    id: t.id.clone(),
                    name: t.name.clone(),
                    progress,
                    total_bytes: t.total_bytes,
                    downloaded_bytes: t.downloaded_bytes,
                    download_speed: t.download_speed,
                    upload_speed: t.upload_speed,
                    peers: t.peers,
                    state: t.state.clone(),
                    added_at: format!("{}s ago", t.added_at.elapsed().as_secs()),
                }
            })
            .collect()
    }

    /// Get the status of a specific torrent by ID.
    #[must_use]
    pub fn get_torrent(&self, id: &str) -> Option<TorrentStatus> {
        self.list_torrents().into_iter().find(|t| t.id == id)
    }

    /// Get the output paths of all completed (seeding) torrents.
    ///
    /// These directories contain the downloaded files and can be scanned
    /// by the music library for automatic import.
    #[must_use]
    pub fn completed_paths(&self) -> Vec<PathBuf> {
        self.torrents
            .iter()
            .filter(|t| matches!(t.state, TorrentState::Seeding))
            .map(|t| t.output_dir.clone())
            .collect()
    }

    /// Number of active (non-paused, non-error) torrents.
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.torrents
            .iter()
            .filter(|t| {
                matches!(
                    t.state,
                    TorrentState::Downloading | TorrentState::Metadata | TorrentState::Seeding
                )
            })
            .count()
    }

    /// Total number of managed torrents.
    #[must_use]
    pub fn total_count(&self) -> usize {
        self.torrents.len()
    }
}

/// Extract the display name (`dn=`) from a magnet URI.
fn extract_magnet_name(uri: &str) -> Option<String> {
    uri.split('&')
        .find(|part| {
            let trimmed = part.trim_start_matches("magnet:?");
            trimmed.starts_with("dn=")
        })
        .and_then(|part| {
            let dn_part = if part.contains("dn=") {
                part.split("dn=").nth(1)
            } else {
                None
            };
            dn_part.map(|s| {
                // URL-decode basic percent encoding for '+' and '%20'.
                s.replace('+', " ").replace("%20", " ")
            })
        })
}

/// Format bytes into a human-readable string.
#[must_use]
#[allow(dead_code)]
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> TorrentConfig {
        TorrentConfig {
            download_dir: std::env::temp_dir().join("hibiki-test-torrents"),
            max_connections: 50,
            dht_enabled: true,
            max_upload_kbps: 0,
            seed_ratio_limit: 2.0,
            auto_import: true,
        }
    }

    #[test]
    fn new_client_has_no_torrents() {
        let client = TorrentClient::new(&test_config());
        assert!(client.list_torrents().is_empty());
        assert!(client.completed_paths().is_empty());
        assert_eq!(client.active_count(), 0);
        assert_eq!(client.total_count(), 0);
    }

    #[tokio::test]
    async fn add_magnet_valid() {
        let mut client = TorrentClient::new(&test_config());

        let result = client
            .add_magnet("magnet:?xt=urn:btih:abc123&dn=Test+Album")
            .await;
        assert!(result.is_ok());

        let id = result.unwrap();
        assert!(id.starts_with("torrent-"));
        assert_eq!(client.list_torrents().len(), 1);
        assert_eq!(client.list_torrents()[0].name, "Test Album");
    }

    #[tokio::test]
    async fn add_magnet_invalid_uri() {
        let mut client = TorrentClient::new(&test_config());

        let result = client.add_magnet("http://not-a-magnet.com").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn add_magnet_without_dn() {
        let mut client = TorrentClient::new(&test_config());

        let result = client
            .add_magnet("magnet:?xt=urn:btih:abc123")
            .await;
        assert!(result.is_ok());

        let torrents = client.list_torrents();
        assert_eq!(torrents.len(), 1);
        assert!(torrents[0].name.starts_with("torrent-"));
    }

    #[tokio::test]
    async fn add_torrent_file_not_found() {
        let mut client = TorrentClient::new(&test_config());

        let result = client
            .add_torrent_file(Path::new("/nonexistent/file.torrent"))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn add_torrent_file_wrong_extension() {
        let dir = tempfile::tempdir().unwrap();
        let bad_file = dir.path().join("not_a_torrent.txt");
        std::fs::write(&bad_file, b"test").unwrap();

        let mut client = TorrentClient::new(&test_config());
        let result = client.add_torrent_file(&bad_file).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn add_torrent_file_valid() {
        let dir = tempfile::tempdir().unwrap();
        let torrent_file = dir.path().join("album.torrent");
        std::fs::write(&torrent_file, b"fake torrent data").unwrap();

        let mut client = TorrentClient::new(&test_config());
        let result = client.add_torrent_file(&torrent_file).await;
        assert!(result.is_ok());

        let torrents = client.list_torrents();
        assert_eq!(torrents.len(), 1);
        assert_eq!(torrents[0].name, "album");
    }

    #[tokio::test]
    async fn pause_and_resume() {
        let mut client = TorrentClient::new(&test_config());

        let id = client
            .add_magnet("magnet:?xt=urn:btih:abc&dn=test")
            .await
            .unwrap();

        client.pause(&id).unwrap();
        assert_eq!(
            client.get_torrent(&id).unwrap().state,
            TorrentState::Paused
        );

        client.resume(&id).unwrap();
        assert_eq!(
            client.get_torrent(&id).unwrap().state,
            TorrentState::Downloading
        );
    }

    #[tokio::test]
    async fn remove_torrent() {
        let mut client = TorrentClient::new(&test_config());

        let id = client
            .add_magnet("magnet:?xt=urn:btih:abc&dn=test")
            .await
            .unwrap();

        client.remove(&id).unwrap();
        assert!(client.list_torrents().is_empty());
    }

    #[tokio::test]
    async fn remove_nonexistent_returns_error() {
        let mut client = TorrentClient::new(&test_config());
        assert!(client.remove("nonexistent").is_err());
    }

    #[tokio::test]
    async fn completed_paths_only_seeding() {
        let mut client = TorrentClient::new(&test_config());

        client
            .add_magnet("magnet:?xt=urn:btih:abc&dn=first")
            .await
            .unwrap();
        client
            .add_magnet("magnet:?xt=urn:btih:def&dn=second")
            .await
            .unwrap();

        assert!(client.completed_paths().is_empty());

        // Simulate one completing.
        client.torrents[0].state = TorrentState::Seeding;

        let completed = client.completed_paths();
        assert_eq!(completed.len(), 1);
        assert!(completed[0].ends_with("first"));
    }

    #[tokio::test]
    async fn unique_ids_for_multiple_torrents() {
        let mut client = TorrentClient::new(&test_config());

        let id1 = client
            .add_magnet("magnet:?xt=urn:btih:a&dn=one")
            .await
            .unwrap();
        let id2 = client
            .add_magnet("magnet:?xt=urn:btih:b&dn=two")
            .await
            .unwrap();
        let id3 = client
            .add_magnet("magnet:?xt=urn:btih:c&dn=three")
            .await
            .unwrap();

        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
    }

    #[test]
    fn extract_magnet_name_with_dn() {
        let name = extract_magnet_name(
            "magnet:?xt=urn:btih:abc&dn=My+Album&tr=udp://tracker.example.com",
        );
        assert_eq!(name, Some("My Album".to_string()));
    }

    #[test]
    fn extract_magnet_name_without_dn() {
        let name = extract_magnet_name("magnet:?xt=urn:btih:abc");
        assert_eq!(name, None);
    }

    #[test]
    fn extract_magnet_name_percent_encoded() {
        let name = extract_magnet_name("magnet:?xt=urn:btih:abc&dn=My%20Album");
        assert_eq!(name, Some("My Album".to_string()));
    }

    #[test]
    fn format_bytes_display() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1_048_576), "1.0 MB");
        assert_eq!(format_bytes(1_073_741_824), "1.0 GB");
        assert_eq!(format_bytes(1_536), "1.5 KB");
    }

    #[test]
    fn torrent_state_display() {
        assert_eq!(TorrentState::Downloading.to_string(), "Downloading");
        assert_eq!(TorrentState::Seeding.to_string(), "Seeding");
        assert_eq!(TorrentState::Paused.to_string(), "Paused");
        assert_eq!(TorrentState::Metadata.to_string(), "Resolving");
        assert_eq!(
            TorrentState::Error("timeout".into()).to_string(),
            "Error: timeout"
        );
    }
}
