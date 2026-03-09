//! BitTorrent client -- download and share hi-fi music.
//!
//! Uses librqbit for the BitTorrent protocol implementation.
//! Features: magnet links, .torrent files, DHT, download management,
//! automatic library import on completion.

use std::path::{Path, PathBuf};

#[derive(thiserror::Error, Debug)]
pub enum TorrentError {
    #[error("invalid magnet uri: {0}")]
    InvalidMagnet(String),
    #[error("torrent file not found: {0}")]
    FileNotFound(PathBuf),
    #[error("download failed: {0}")]
    Download(String),
    #[error("client error: {0}")]
    Client(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, TorrentError>;

/// Status of an individual torrent.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TorrentStatus {
    /// Display name of the torrent.
    pub name: String,
    /// Download progress from 0.0 to 1.0.
    pub progress: f32,
    /// Current download speed in bytes per second.
    pub download_speed: u64,
    /// Current upload speed in bytes per second.
    pub upload_speed: u64,
    /// Number of connected peers.
    pub peers: u32,
    /// Current state of the torrent.
    pub state: TorrentState,
}

/// Possible states of a torrent.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum TorrentState {
    Downloading,
    Seeding,
    Paused,
    Checking,
    Error(String),
}

/// Internal record for a managed torrent.
struct ManagedTorrent {
    id: String,
    name: String,
    state: TorrentState,
    output_dir: PathBuf,
}

/// BitTorrent client for downloading music.
///
/// Wraps librqbit and manages torrents in the configured download directory.
/// Completed downloads can be enumerated via [`completed_paths`] for automatic
/// import into the music library.
pub struct TorrentClient {
    download_dir: PathBuf,
    torrents: Vec<ManagedTorrent>,
    next_id: u64,
    // TODO: librqbit::Session handle
}

impl TorrentClient {
    /// Create a new torrent client that saves downloads to `download_dir`.
    ///
    /// The directory is created if it does not exist.
    #[must_use]
    pub fn new(download_dir: PathBuf) -> Self {
        tracing::info!(dir = %download_dir.display(), "initialising torrent client");

        if !download_dir.exists() {
            if let Err(e) = std::fs::create_dir_all(&download_dir) {
                tracing::warn!(
                    dir = %download_dir.display(),
                    error = %e,
                    "failed to create download directory"
                );
            }
        }

        Self {
            download_dir,
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
        let name = extract_magnet_name(uri).unwrap_or_else(|| format!("torrent-{}", self.next_id));

        let id = format!("torrent-{}", self.next_id);
        self.next_id += 1;

        let output_dir = self.download_dir.join(&name);

        self.torrents.push(ManagedTorrent {
            id: id.clone(),
            name,
            state: TorrentState::Downloading,
            output_dir,
        });

        // TODO: Create librqbit session and add magnet
        // let session = librqbit::Session::new(output_dir).await?;
        // session.add_magnet(uri).await?;

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

        let output_dir = self.download_dir.join(&name);

        self.torrents.push(ManagedTorrent {
            id: id.clone(),
            name,
            state: TorrentState::Downloading,
            output_dir,
        });

        // TODO: Read .torrent file and add to librqbit session
        // let data = tokio::fs::read(path).await?;
        // session.add_torrent_file(data).await?;

        tracing::info!(id = %id, "torrent file added");
        Ok(id)
    }

    /// List the status of all managed torrents.
    #[must_use]
    pub fn list_torrents(&self) -> Vec<TorrentStatus> {
        self.torrents
            .iter()
            .map(|t| {
                // TODO: Query librqbit for real stats
                TorrentStatus {
                    name: t.name.clone(),
                    progress: match &t.state {
                        TorrentState::Seeding => 1.0,
                        _ => 0.0,
                    },
                    download_speed: 0,
                    upload_speed: 0,
                    peers: 0,
                    state: t.state.clone(),
                }
            })
            .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_client_has_no_torrents() {
        let dir = tempfile::tempdir().unwrap();
        let client = TorrentClient::new(dir.path().to_path_buf());
        assert!(client.list_torrents().is_empty());
        assert!(client.completed_paths().is_empty());
    }

    #[tokio::test]
    async fn add_magnet_valid() {
        let dir = tempfile::tempdir().unwrap();
        let mut client = TorrentClient::new(dir.path().to_path_buf());

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
        let dir = tempfile::tempdir().unwrap();
        let mut client = TorrentClient::new(dir.path().to_path_buf());

        let result = client.add_magnet("http://not-a-magnet.com").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn add_magnet_without_dn() {
        let dir = tempfile::tempdir().unwrap();
        let mut client = TorrentClient::new(dir.path().to_path_buf());

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
        let dir = tempfile::tempdir().unwrap();
        let mut client = TorrentClient::new(dir.path().to_path_buf());

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

        let mut client = TorrentClient::new(dir.path().to_path_buf());
        let result = client.add_torrent_file(&bad_file).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn add_torrent_file_valid() {
        let dir = tempfile::tempdir().unwrap();
        let torrent_file = dir.path().join("album.torrent");
        std::fs::write(&torrent_file, b"fake torrent data").unwrap();

        let mut client = TorrentClient::new(dir.path().to_path_buf());
        let result = client.add_torrent_file(&torrent_file).await;
        assert!(result.is_ok());

        let torrents = client.list_torrents();
        assert_eq!(torrents.len(), 1);
        assert_eq!(torrents[0].name, "album");
    }

    #[tokio::test]
    async fn completed_paths_only_seeding() {
        let dir = tempfile::tempdir().unwrap();
        let mut client = TorrentClient::new(dir.path().to_path_buf());

        // Add two torrents.
        client
            .add_magnet("magnet:?xt=urn:btih:abc&dn=first")
            .await
            .unwrap();
        client
            .add_magnet("magnet:?xt=urn:btih:def&dn=second")
            .await
            .unwrap();

        // No completed yet.
        assert!(client.completed_paths().is_empty());

        // Simulate one completing.
        client.torrents[0].state = TorrentState::Seeding;

        let completed = client.completed_paths();
        assert_eq!(completed.len(), 1);
        assert!(completed[0].ends_with("first"));
    }

    #[tokio::test]
    async fn unique_ids_for_multiple_torrents() {
        let dir = tempfile::tempdir().unwrap();
        let mut client = TorrentClient::new(dir.path().to_path_buf());

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
        let name =
            extract_magnet_name("magnet:?xt=urn:btih:abc&dn=My+Album&tr=udp://tracker.example.com");
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
}
