//! Music library management -- scanning, metadata extraction, indexing, playlists.
//!
//! Scans configured music directories, extracts tags (ID3v2, Vorbis comments,
//! FLAC tags, etc.) via lofty, and maintains a searchable index of the user's
//! collection. Supports playlists and fuzzy search.

use crate::audio::{self, Track};
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::tag::Accessor;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(thiserror::Error, Debug)]
#[allow(dead_code)]
pub enum LibraryError {
    #[error("scan failed: {0}")]
    Scan(String),
    #[error("directory not found: {0}")]
    DirNotFound(PathBuf),
    #[error("metadata extraction failed: {0}")]
    Metadata(String),
    #[error("playlist error: {0}")]
    Playlist(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, LibraryError>;

/// A named playlist -- an ordered list of track indices into the library.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
pub struct Playlist {
    pub name: String,
    pub tracks: Vec<PathBuf>,
}

#[allow(dead_code)]
impl Playlist {
    /// Create a new empty playlist.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            tracks: Vec::new(),
        }
    }

    /// Add a track path to the playlist.
    pub fn add(&mut self, path: PathBuf) {
        self.tracks.push(path);
    }

    /// Remove a track by index.
    pub fn remove(&mut self, index: usize) -> Option<PathBuf> {
        if index < self.tracks.len() {
            Some(self.tracks.remove(index))
        } else {
            None
        }
    }

    /// Number of tracks in the playlist.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    /// Whether the playlist is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }
}

/// Library statistics.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LibraryStats {
    pub total_tracks: usize,
    pub total_artists: usize,
    pub total_albums: usize,
    pub total_duration: Duration,
    pub scan_dirs: Vec<PathBuf>,
}

/// Music library: scans directories for audio files and provides search.
#[allow(dead_code)]
pub struct Library {
    tracks: Vec<Track>,
    playlists: Vec<Playlist>,
    scan_dirs: Vec<PathBuf>,
}

#[allow(dead_code)]
impl Library {
    /// Create a new empty library.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tracks: Vec::new(),
            playlists: Vec::new(),
            scan_dirs: Vec::new(),
        }
    }

    /// Scan a directory recursively for audio files and add them to the library.
    ///
    /// Extracts metadata from audio files using lofty for tag reading, falling
    /// back to filename-based metadata when tags are unavailable.
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
            if !audio::is_audio_file(&path) {
                continue;
            }

            // Skip files already in the library.
            if self.tracks.iter().any(|t| t.path == path) {
                tracing::trace!(path = %path.display(), "track already indexed, skipping");
                continue;
            }

            let track = extract_metadata(&path);
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

    /// Search tracks by a case-insensitive query string (fuzzy match).
    ///
    /// Matches against title, artist, and album fields. Results are scored
    /// by match quality: exact matches score higher than substring matches.
    #[must_use]
    pub fn search(&self, query: &str) -> Vec<&Track> {
        if query.is_empty() {
            return self.tracks.iter().collect();
        }

        let query_lower = query.to_lowercase();
        let query_words: Vec<&str> = query_lower.split_whitespace().collect();

        let mut scored: Vec<(&Track, u32)> = self
            .tracks
            .iter()
            .filter_map(|track| {
                let mut score = 0u32;

                let title_lower = track.title.to_lowercase();
                let artist_lower = track
                    .artist
                    .as_deref()
                    .map(str::to_lowercase)
                    .unwrap_or_default();
                let album_lower = track
                    .album
                    .as_deref()
                    .map(str::to_lowercase)
                    .unwrap_or_default();

                // Exact match scoring.
                if title_lower == query_lower {
                    score += 100;
                } else if title_lower.contains(&query_lower) {
                    score += 50;
                }

                if artist_lower == query_lower {
                    score += 80;
                } else if artist_lower.contains(&query_lower) {
                    score += 40;
                }

                if album_lower == query_lower {
                    score += 60;
                } else if album_lower.contains(&query_lower) {
                    score += 30;
                }

                // Word-level matching for multi-word queries.
                // All words must be present for a match.
                if score == 0 && query_words.len() > 1 {
                    let searchable =
                        format!("{title_lower} {artist_lower} {album_lower}");
                    let all_match = query_words.iter().all(|w| searchable.contains(w));
                    if all_match {
                        score += 10 * query_words.len() as u32;
                    }
                }

                if score > 0 { Some((track, score)) } else { None }
            })
            .collect();

        scored.sort_by(|a, b| b.1.cmp(&a.1));
        scored.into_iter().map(|(t, _)| t).collect()
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
    ///
    /// Results are sorted by disc number then track number.
    #[must_use]
    pub fn by_album(&self, album: &str) -> Vec<&Track> {
        let album_lower = album.to_lowercase();
        let mut tracks: Vec<&Track> = self
            .tracks
            .iter()
            .filter(|t| {
                t.album
                    .as_deref()
                    .is_some_and(|a| a.to_lowercase() == album_lower)
            })
            .collect();

        tracks.sort_by(|a, b| {
            a.disc_number
                .unwrap_or(1)
                .cmp(&b.disc_number.unwrap_or(1))
                .then(a.track_number.unwrap_or(0).cmp(&b.track_number.unwrap_or(0)))
        });

        tracks
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

    /// Get a sorted, deduplicated list of all genres.
    #[must_use]
    pub fn all_genres(&self) -> Vec<&str> {
        let mut genres: Vec<&str> = self
            .tracks
            .iter()
            .filter_map(|t| t.genre.as_deref())
            .collect();
        genres.sort_unstable();
        genres.dedup();
        genres
    }

    /// Get a slice of all tracks in the library.
    #[must_use]
    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    /// Get the track at a specific index.
    #[must_use]
    pub fn get_track(&self, index: usize) -> Option<&Track> {
        self.tracks.get(index)
    }

    /// Find a track by its file path.
    #[must_use]
    pub fn find_by_path(&self, path: &Path) -> Option<&Track> {
        self.tracks.iter().find(|t| t.path == path)
    }

    /// Total number of tracks in the library.
    #[must_use]
    pub fn track_count(&self) -> usize {
        self.tracks.len()
    }

    /// Get library statistics.
    #[must_use]
    pub fn stats(&self) -> LibraryStats {
        let total_duration = self
            .tracks
            .iter()
            .filter_map(|t| t.duration)
            .sum();

        LibraryStats {
            total_tracks: self.tracks.len(),
            total_artists: self.all_artists().len(),
            total_albums: self.all_albums().len(),
            total_duration,
            scan_dirs: self.scan_dirs.clone(),
        }
    }

    // --- Playlist management ---

    /// Create a new playlist and return its index.
    pub fn create_playlist(&mut self, name: impl Into<String>) -> usize {
        let playlist = Playlist::new(name);
        self.playlists.push(playlist);
        self.playlists.len() - 1
    }

    /// Get a reference to a playlist by index.
    #[must_use]
    pub fn get_playlist(&self, index: usize) -> Option<&Playlist> {
        self.playlists.get(index)
    }

    /// Get a mutable reference to a playlist by index.
    pub fn get_playlist_mut(&mut self, index: usize) -> Option<&mut Playlist> {
        self.playlists.get_mut(index)
    }

    /// Get all playlists.
    #[must_use]
    pub fn playlists(&self) -> &[Playlist] {
        &self.playlists
    }

    /// Remove a playlist by index.
    pub fn remove_playlist(&mut self, index: usize) -> Option<Playlist> {
        if index < self.playlists.len() {
            Some(self.playlists.remove(index))
        } else {
            None
        }
    }

    /// Find a playlist by name.
    #[must_use]
    pub fn find_playlist(&self, name: &str) -> Option<(usize, &Playlist)> {
        self.playlists
            .iter()
            .enumerate()
            .find(|(_, p)| p.name == name)
    }

    /// Resolve a playlist to a list of tracks (skipping paths not in the library).
    #[must_use]
    pub fn resolve_playlist(&self, playlist: &Playlist) -> Vec<&Track> {
        playlist
            .tracks
            .iter()
            .filter_map(|p| self.find_by_path(p))
            .collect()
    }
}

impl Default for Library {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract metadata from an audio file.
///
/// Uses lofty for tag reading (ID3v2, Vorbis comments, FLAC tags, MP4, etc.).
/// Falls back to filename-based metadata if tag reading fails.
pub fn extract_metadata(path: &Path) -> Track {
    let codec = audio::detect_codec(path)
        .unwrap_or("unknown")
        .to_string();

    // Try lofty tag extraction first.
    if let Ok(tagged_file) = lofty::read_from_path(path) {
        let tag = tagged_file.primary_tag().or_else(|| tagged_file.first_tag());
        let properties = tagged_file.properties();

        let (title, artist, album, track_number, disc_number, year, genre) = if let Some(tag) = tag
        {
            (
                tag.title().map(|s| s.to_string()),
                tag.artist().map(|s| s.to_string()),
                tag.album().map(|s| s.to_string()),
                tag.track(),
                tag.disk(),
                tag.year(),
                tag.genre().map(|s| s.to_string()),
            )
        } else {
            (None, None, None, None, None, None, None)
        };

        let duration = properties.duration();
        let sample_rate = properties.sample_rate();
        let bit_depth = properties.bit_depth().map(u32::from);

        // Fall back to filename for title if tag is missing.
        let title = title.unwrap_or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Unknown")
                .to_string()
        });

        return Track {
            path: path.to_path_buf(),
            title,
            artist,
            album,
            duration: if duration.is_zero() {
                None
            } else {
                Some(duration)
            },
            track_number,
            disc_number,
            year,
            genre,
            codec,
            sample_rate,
            bit_depth,
        };
    }

    // Fallback: extract metadata from filename.
    extract_metadata_from_filename(path, &codec)
}

/// Fallback metadata extraction from file path when tag reading fails.
fn extract_metadata_from_filename(path: &Path, codec: &str) -> Track {
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
        duration: None,
        track_number: None,
        disc_number: None,
        year: None,
        genre: None,
        codec: codec.to_string(),
        sample_rate: None,
        bit_depth: None,
    }
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
            disc_number: None,
            year: None,
            genre: None,
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
        assert_eq!(lib.track_count(), 0);
    }

    #[test]
    fn search_by_title() {
        let mut lib = Library::new();
        lib.tracks
            .push(make_track("Bohemian Rhapsody", Some("Queen"), Some("News of the World")));
        lib.tracks
            .push(make_track("Stairway to Heaven", Some("Led Zeppelin"), Some("IV")));
        lib.tracks
            .push(make_track("Hotel California", Some("Eagles"), Some("Hotel California")));

        let results = lib.search("bohemian");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Bohemian Rhapsody");
    }

    #[test]
    fn search_by_artist() {
        let mut lib = Library::new();
        lib.tracks.push(make_track("Song A", Some("Queen"), None));
        lib.tracks
            .push(make_track("Song B", Some("Beatles"), None));

        let results = lib.search("queen");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].artist.as_deref(), Some("Queen"));
    }

    #[test]
    fn search_by_album() {
        let mut lib = Library::new();
        lib.tracks
            .push(make_track("Song A", None, Some("Dark Side")));
        lib.tracks
            .push(make_track("Song B", None, Some("The Wall")));

        let results = lib.search("dark");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].album.as_deref(), Some("Dark Side"));
    }

    #[test]
    fn search_case_insensitive() {
        let mut lib = Library::new();
        lib.tracks
            .push(make_track("LOUD TITLE", Some("quiet artist"), None));

        assert_eq!(lib.search("loud").len(), 1);
        assert_eq!(lib.search("LOUD").len(), 1);
        assert_eq!(lib.search("Loud").len(), 1);
    }

    #[test]
    fn search_multi_word() {
        let mut lib = Library::new();
        lib.tracks
            .push(make_track("Stairway to Heaven", Some("Led Zeppelin"), None));
        lib.tracks
            .push(make_track("Heaven and Hell", Some("Black Sabbath"), None));

        let results = lib.search("led heaven");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Stairway to Heaven");
    }

    #[test]
    fn search_empty_query_returns_all() {
        let mut lib = Library::new();
        lib.tracks.push(make_track("A", None, None));
        lib.tracks.push(make_track("B", None, None));
        assert_eq!(lib.search("").len(), 2);
    }

    #[test]
    fn search_no_results() {
        let mut lib = Library::new();
        lib.tracks
            .push(make_track("Song", Some("Artist"), None));

        assert!(lib.search("nonexistent").is_empty());
    }

    #[test]
    fn by_artist_exact_match() {
        let mut lib = Library::new();
        lib.tracks.push(make_track("A", Some("Queen"), None));
        lib.tracks.push(make_track("B", Some("Queen"), None));
        lib.tracks
            .push(make_track("C", Some("Beatles"), None));

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
        lib.tracks
            .push(make_track("A", None, Some("Album X")));
        lib.tracks
            .push(make_track("B", None, Some("Album X")));
        lib.tracks
            .push(make_track("C", None, Some("Album Y")));

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
        lib.tracks
            .push(make_track("A", Some("Artist"), Some("Bravo")));
        lib.tracks
            .push(make_track("B", Some("Artist"), Some("Alpha")));
        lib.tracks
            .push(make_track("C", Some("Artist"), Some("Alpha")));
        lib.tracks.push(make_track("D", None, None));

        let albums = lib.all_albums();
        assert_eq!(albums.len(), 2);
        assert_eq!(albums[0].0, "Alpha");
        assert_eq!(albums[1].0, "Bravo");
    }

    #[test]
    fn all_genres() {
        let mut lib = Library::new();
        let mut t1 = make_track("A", None, None);
        t1.genre = Some("Rock".into());
        let mut t2 = make_track("B", None, None);
        t2.genre = Some("Jazz".into());
        let mut t3 = make_track("C", None, None);
        t3.genre = Some("Rock".into());
        lib.tracks.push(t1);
        lib.tracks.push(t2);
        lib.tracks.push(t3);

        let genres = lib.all_genres();
        assert_eq!(genres, vec!["Jazz", "Rock"]);
    }

    #[test]
    fn stats() {
        let mut lib = Library::new();
        lib.tracks
            .push(make_track("A", Some("Artist1"), Some("Album1")));
        lib.tracks
            .push(make_track("B", Some("Artist2"), Some("Album2")));

        let stats = lib.stats();
        assert_eq!(stats.total_tracks, 2);
        assert_eq!(stats.total_artists, 2);
        assert_eq!(stats.total_albums, 2);
        assert_eq!(stats.total_duration, Duration::from_secs(400));
    }

    #[test]
    fn playlist_create_and_manage() {
        let mut lib = Library::new();
        let idx = lib.create_playlist("Favourites");
        assert_eq!(idx, 0);

        let pl = lib.get_playlist_mut(idx).unwrap();
        pl.add(PathBuf::from("/music/a.flac"));
        pl.add(PathBuf::from("/music/b.flac"));
        assert_eq!(pl.len(), 2);

        pl.remove(0);
        assert_eq!(pl.len(), 1);
    }

    #[test]
    fn playlist_find_by_name() {
        let mut lib = Library::new();
        lib.create_playlist("Rock");
        lib.create_playlist("Jazz");

        let (idx, pl) = lib.find_playlist("Jazz").unwrap();
        assert_eq!(idx, 1);
        assert_eq!(pl.name, "Jazz");
    }

    #[test]
    fn remove_playlist() {
        let mut lib = Library::new();
        lib.create_playlist("A");
        lib.create_playlist("B");
        let removed = lib.remove_playlist(0);
        assert_eq!(removed.unwrap().name, "A");
        assert_eq!(lib.playlists().len(), 1);
    }

    #[test]
    fn extract_metadata_from_filename_with_artist() {
        let path = Path::new("/music/Artist Name - Song Title.flac");
        let track = extract_metadata_from_filename(path, "flac");
        assert_eq!(track.title, "Song Title");
        assert_eq!(track.artist.as_deref(), Some("Artist Name"));
        assert_eq!(track.album.as_deref(), Some("music"));
        assert_eq!(track.codec, "flac");
    }

    #[test]
    fn extract_metadata_from_filename_without_artist() {
        let path = Path::new("/music/albums/Just A Title.mp3");
        let track = extract_metadata_from_filename(path, "mp3");
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

    #[test]
    fn find_by_path() {
        let mut lib = Library::new();
        let track = make_track("A", None, None);
        let path = track.path.clone();
        lib.tracks.push(track);

        assert!(lib.find_by_path(&path).is_some());
        assert!(lib.find_by_path(Path::new("/nope")).is_none());
    }
}
