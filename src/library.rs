//! Music library management -- scanning, metadata extraction, indexing.
//!
//! Scans configured music directories, extracts tags (artist, album, track),
//! and maintains a searchable index of the user's collection.

use crate::audio::{self, AudioError, Track};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(thiserror::Error, Debug)]
pub enum LibraryError {
    #[error("scan failed: {0}")]
    Scan(String),
    #[error("directory not found: {0}")]
    DirNotFound(PathBuf),
    #[error("metadata extraction failed: {0}")]
    Metadata(String),
    #[error(transparent)]
    Audio(#[from] AudioError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, LibraryError>;

/// Music library: scans directories for audio files and provides search.
pub struct Library {
    tracks: Vec<Track>,
    scan_dirs: Vec<PathBuf>,
}

impl Library {
    /// Create a new empty library.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tracks: Vec::new(),
            scan_dirs: Vec::new(),
        }
    }

    /// Scan a directory recursively for audio files and add them to the library.
    ///
    /// Returns the count of newly discovered tracks. Tracks already in the library
    /// (matched by path) are skipped.
    pub async fn scan(&mut self, dir: &Path) -> Result<usize> {
        if !dir.exists() {
            return Err(LibraryError::DirNotFound(dir.to_path_buf()));
        }

        if !dir.is_dir() {
            return Err(LibraryError::Scan(format!(
                "path is not a directory: {}",
                dir.display()
            )));
        }

        tracing::info!(dir = %dir.display(), "scanning directory for audio files");

        if !self.scan_dirs.contains(&dir.to_path_buf()) {
            self.scan_dirs.push(dir.to_path_buf());
        }

        let mut count = 0;
        let mut entries = tokio::fs::read_dir(dir).await.map_err(|e| {
            LibraryError::Scan(format!("failed to read directory {}: {e}", dir.display()))
        })?;

        while let Some(entry) = entries.next_entry().await.map_err(|e| {
            LibraryError::Scan(format!("failed to read entry: {e}"))
        })? {
            let path = entry.path();

            if path.is_dir() {
                // Recurse into subdirectories using a boxed future to allow recursion.
                count += Box::pin(self.scan(&path)).await?;
                continue;
            }

            // Skip non-audio files.
            let Some(codec) = audio::detect_codec(&path) else {
                continue;
            };

            // Skip files already in the library.
            if self.tracks.iter().any(|t| t.path == path) {
                tracing::trace!(path = %path.display(), "track already indexed, skipping");
                continue;
            }

            let track = extract_metadata(&path, codec);
            tracing::debug!(
                title = %track.title,
                artist = ?track.artist,
                codec = %track.codec,
                "discovered track"
            );
            self.tracks.push(track);
            count += 1;
        }

        tracing::info!(count, "scan complete");
        Ok(count)
    }

    /// Search tracks by a case-insensitive query string.
    ///
    /// Matches against title, artist, and album fields.
    #[must_use]
    pub fn search(&self, query: &str) -> Vec<&Track> {
        let query_lower = query.to_lowercase();
        self.tracks
            .iter()
            .filter(|track| {
                track.title.to_lowercase().contains(&query_lower)
                    || track
                        .artist
                        .as_deref()
                        .is_some_and(|a| a.to_lowercase().contains(&query_lower))
                    || track
                        .album
                        .as_deref()
                        .is_some_and(|a| a.to_lowercase().contains(&query_lower))
            })
            .collect()
    }

    /// Get all tracks by a specific artist (case-insensitive match).
    #[must_use]
    pub fn by_artist(&self, artist: &str) -> Vec<&Track> {
        let artist_lower = artist.to_lowercase();
        self.tracks
            .iter()
            .filter(|t| {
                t.artist
                    .as_deref()
                    .is_some_and(|a| a.to_lowercase() == artist_lower)
            })
            .collect()
    }

    /// Get all tracks in a specific album (case-insensitive match).
    #[must_use]
    pub fn by_album(&self, album: &str) -> Vec<&Track> {
        let album_lower = album.to_lowercase();
        self.tracks
            .iter()
            .filter(|t| {
                t.album
                    .as_deref()
                    .is_some_and(|a| a.to_lowercase() == album_lower)
            })
            .collect()
    }

    /// Get a sorted, deduplicated list of all artists in the library.
    #[must_use]
    pub fn all_artists(&self) -> Vec<&str> {
        let mut artists: Vec<&str> = self
            .tracks
            .iter()
            .filter_map(|t| t.artist.as_deref())
            .collect();
        artists.sort_unstable();
        artists.dedup();
        artists
    }

    /// Get a sorted, deduplicated list of all albums with their artist.
    ///
    /// Returns tuples of `(album_name, optional_artist)`.
    #[must_use]
    pub fn all_albums(&self) -> Vec<(&str, Option<&str>)> {
        let mut albums: Vec<(&str, Option<&str>)> = self
            .tracks
            .iter()
            .filter_map(|t| {
                t.album
                    .as_deref()
                    .map(|album| (album, t.artist.as_deref()))
            })
            .collect();
        albums.sort_unstable();
        albums.dedup();
        albums
    }

    /// Get a slice of all tracks in the library.
    #[must_use]
    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }
}

impl Default for Library {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract metadata from an audio file.
///
/// Currently builds metadata from the file path (filename-based).
/// TODO: Use symphonia's metadata probing for proper tag extraction.
fn extract_metadata(path: &Path, codec: &str) -> Track {
    let file_stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown")
        .to_string();

    // Attempt to parse "Artist - Title" from filename.
    let (artist, title) = if let Some(sep_pos) = file_stem.find(" - ") {
        (
            Some(file_stem[..sep_pos].to_string()),
            file_stem[sep_pos + 3..].to_string(),
        )
    } else {
        (None, file_stem)
    };

    // Attempt to infer album from the parent directory name.
    let album = path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .map(String::from);

    Track {
        path: path.to_path_buf(),
        title,
        artist,
        album,
        duration: None, // TODO: extract via symphonia probe
        track_number: None,
        codec: codec.to_string(),
        sample_rate: None,
        bit_depth: None,
    }
}

/// Detect the codec name from a file extension.
///
/// This is a convenience re-export of [`audio::detect_codec`].
#[must_use]
pub fn detect_codec(path: &Path) -> Option<&'static str> {
    audio::detect_codec(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_track(title: &str, artist: Option<&str>, album: Option<&str>) -> Track {
        Track {
            path: PathBuf::from(format!("/music/{title}.flac")),
            title: title.to_string(),
            artist: artist.map(String::from),
            album: album.map(String::from),
            duration: Some(Duration::from_secs(200)),
            track_number: None,
            codec: "flac".to_string(),
            sample_rate: Some(44100),
            bit_depth: Some(16),
        }
    }

    #[test]
    fn new_library_is_empty() {
        let lib = Library::new();
        assert!(lib.tracks().is_empty());
        assert!(lib.all_artists().is_empty());
        assert!(lib.all_albums().is_empty());
    }

    #[test]
    fn search_by_title() {
        let mut lib = Library::new();
        lib.tracks.push(make_track("Bohemian Rhapsody", Some("Queen"), Some("News of the World")));
        lib.tracks.push(make_track("Stairway to Heaven", Some("Led Zeppelin"), Some("IV")));
        lib.tracks.push(make_track("Hotel California", Some("Eagles"), Some("Hotel California")));

        let results = lib.search("bohemian");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Bohemian Rhapsody");
    }

    #[test]
    fn search_by_artist() {
        let mut lib = Library::new();
        lib.tracks.push(make_track("Song A", Some("Queen"), None));
        lib.tracks.push(make_track("Song B", Some("Beatles"), None));

        let results = lib.search("queen");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].artist.as_deref(), Some("Queen"));
    }

    #[test]
    fn search_by_album() {
        let mut lib = Library::new();
        lib.tracks.push(make_track("Song A", None, Some("Dark Side")));
        lib.tracks.push(make_track("Song B", None, Some("The Wall")));

        let results = lib.search("dark");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].album.as_deref(), Some("Dark Side"));
    }

    #[test]
    fn search_case_insensitive() {
        let mut lib = Library::new();
        lib.tracks.push(make_track("LOUD TITLE", Some("quiet artist"), None));

        assert_eq!(lib.search("loud").len(), 1);
        assert_eq!(lib.search("LOUD").len(), 1);
        assert_eq!(lib.search("Loud").len(), 1);
    }

    #[test]
    fn search_no_results() {
        let mut lib = Library::new();
        lib.tracks.push(make_track("Song", Some("Artist"), None));

        assert!(lib.search("nonexistent").is_empty());
    }

    #[test]
    fn by_artist_exact_match() {
        let mut lib = Library::new();
        lib.tracks.push(make_track("A", Some("Queen"), None));
        lib.tracks.push(make_track("B", Some("Queen"), None));
        lib.tracks.push(make_track("C", Some("Beatles"), None));

        let results = lib.by_artist("Queen");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn by_artist_case_insensitive() {
        let mut lib = Library::new();
        lib.tracks.push(make_track("A", Some("Queen"), None));

        assert_eq!(lib.by_artist("queen").len(), 1);
        assert_eq!(lib.by_artist("QUEEN").len(), 1);
    }

    #[test]
    fn by_album_exact_match() {
        let mut lib = Library::new();
        lib.tracks.push(make_track("A", None, Some("Album X")));
        lib.tracks.push(make_track("B", None, Some("Album X")));
        lib.tracks.push(make_track("C", None, Some("Album Y")));

        let results = lib.by_album("Album X");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn all_artists_sorted_and_deduped() {
        let mut lib = Library::new();
        lib.tracks.push(make_track("A", Some("Zed"), None));
        lib.tracks.push(make_track("B", Some("Alpha"), None));
        lib.tracks.push(make_track("C", Some("Alpha"), None));
        lib.tracks.push(make_track("D", None, None));

        let artists = lib.all_artists();
        assert_eq!(artists, vec!["Alpha", "Zed"]);
    }

    #[test]
    fn all_albums_sorted_and_deduped() {
        let mut lib = Library::new();
        lib.tracks.push(make_track("A", Some("Artist"), Some("Bravo")));
        lib.tracks.push(make_track("B", Some("Artist"), Some("Alpha")));
        lib.tracks.push(make_track("C", Some("Artist"), Some("Alpha")));
        lib.tracks.push(make_track("D", None, None));

        let albums = lib.all_albums();
        assert_eq!(albums.len(), 2);
        assert_eq!(albums[0].0, "Alpha");
        assert_eq!(albums[1].0, "Bravo");
    }

    #[test]
    fn extract_metadata_from_filename_with_artist() {
        let path = Path::new("/music/Artist Name - Song Title.flac");
        let track = extract_metadata(path, "flac");
        assert_eq!(track.title, "Song Title");
        assert_eq!(track.artist.as_deref(), Some("Artist Name"));
        assert_eq!(track.album.as_deref(), Some("music"));
        assert_eq!(track.codec, "flac");
    }

    #[test]
    fn extract_metadata_from_filename_without_artist() {
        let path = Path::new("/music/albums/Just A Title.mp3");
        let track = extract_metadata(path, "mp3");
        assert_eq!(track.title, "Just A Title");
        assert!(track.artist.is_none());
        assert_eq!(track.album.as_deref(), Some("albums"));
    }

    #[tokio::test]
    async fn scan_nonexistent_dir_returns_error() {
        let mut lib = Library::new();
        let result = lib.scan(Path::new("/nonexistent/path")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn scan_empty_dir_returns_zero() {
        let dir = tempfile::tempdir().unwrap();
        let mut lib = Library::new();
        let count = lib.scan(dir.path()).await.unwrap();
        assert_eq!(count, 0);
        assert!(lib.tracks().is_empty());
    }

    #[tokio::test]
    async fn scan_finds_audio_files() {
        let dir = tempfile::tempdir().unwrap();

        // Create fake audio files (just empty files with audio extensions).
        std::fs::write(dir.path().join("track1.flac"), b"").unwrap();
        std::fs::write(dir.path().join("track2.mp3"), b"").unwrap();
        std::fs::write(dir.path().join("notes.txt"), b"").unwrap();
        std::fs::write(dir.path().join("cover.jpg"), b"").unwrap();

        let mut lib = Library::new();
        let count = lib.scan(dir.path()).await.unwrap();
        assert_eq!(count, 2);
        assert_eq!(lib.tracks().len(), 2);

        let titles: Vec<&str> = lib.tracks().iter().map(|t| t.title.as_str()).collect();
        assert!(titles.contains(&"track1"));
        assert!(titles.contains(&"track2"));
    }

    #[tokio::test]
    async fn scan_skips_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("song.flac"), b"").unwrap();

        let mut lib = Library::new();
        let count1 = lib.scan(dir.path()).await.unwrap();
        assert_eq!(count1, 1);

        let count2 = lib.scan(dir.path()).await.unwrap();
        assert_eq!(count2, 0);
        assert_eq!(lib.tracks().len(), 1);
    }

    #[tokio::test]
    async fn scan_recurses_subdirectories() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("subdir");
        std::fs::create_dir(&sub).unwrap();

        std::fs::write(dir.path().join("top.flac"), b"").unwrap();
        std::fs::write(sub.join("nested.mp3"), b"").unwrap();

        let mut lib = Library::new();
        let count = lib.scan(dir.path()).await.unwrap();
        assert_eq!(count, 2);
    }
}
