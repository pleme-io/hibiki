//! Audio playback engine -- rodio + symphonia for hi-fi codec support.
//!
//! Supports: FLAC, ALAC, WAV, AIFF, OGG Vorbis, MP3, AAC
//! Features: gapless playback, sample rate conversion, queue management
//!
//! Uses oto state machines (Player, Queue) with rodio for actual output
//! and symphonia for codec decoding.

use crate::config::AudioConfig;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(thiserror::Error, Debug)]
pub enum AudioError {
    #[error("playback failed: {0}")]
    Playback(String),
    #[error("codec not supported: {0}")]
    UnsupportedCodec(String),
    #[error("file not found: {0}")]
    FileNotFound(PathBuf),
    #[error("device error: {0}")]
    Device(String),
}

pub type Result<T> = std::result::Result<T, AudioError>;

/// A track in the library.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Track {
    pub path: PathBuf,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration: Option<Duration>,
    pub track_number: Option<u32>,
    pub codec: String,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u32>,
}

/// Audio engine that manages playback state and output.
pub struct AudioEngine {
    state: PlaybackState,
    queue: Vec<Track>,
    current_index: Option<usize>,
    volume: f32,
    config: AudioConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Stopped,
    Playing,
    Paused,
}

impl AudioEngine {
    /// Create a new audio engine with the given configuration.
    ///
    /// Initialises the engine in the `Stopped` state with an empty queue.
    #[must_use]
    pub fn new(config: &AudioConfig) -> Self {
        tracing::debug!(
            sample_rate = config.sample_rate,
            buffer_size = config.buffer_size,
            gapless = config.gapless,
            "initialising audio engine"
        );

        Self {
            state: PlaybackState::Stopped,
            queue: Vec::new(),
            current_index: None,
            volume: 1.0,
            config: config.clone(),
        }
    }

    /// Start or resume playback.
    ///
    /// If the engine is stopped, begins playing the first track in the queue.
    /// If paused, resumes playback of the current track.
    /// If already playing, this is a no-op.
    pub fn play(&mut self) -> Result<()> {
        match self.state {
            PlaybackState::Playing => {
                tracing::trace!("already playing, ignoring play request");
                Ok(())
            }
            PlaybackState::Paused => {
                tracing::info!("resuming playback");
                self.state = PlaybackState::Playing;
                // TODO: Resume rodio sink
                Ok(())
            }
            PlaybackState::Stopped => {
                if self.queue.is_empty() {
                    tracing::warn!("play requested but queue is empty");
                    return Err(AudioError::Playback("queue is empty".into()));
                }

                let index = self.current_index.unwrap_or(0);
                let track = &self.queue[index];

                if !track.path.exists() {
                    return Err(AudioError::FileNotFound(track.path.clone()));
                }

                tracing::info!(
                    title = %track.title,
                    artist = ?track.artist,
                    codec = %track.codec,
                    "starting playback"
                );

                self.current_index = Some(index);
                self.state = PlaybackState::Playing;
                // TODO: Open file with symphonia, decode, pipe to rodio sink
                Ok(())
            }
        }
    }

    /// Pause playback.
    ///
    /// If playing, pauses the current track. Otherwise this is a no-op.
    pub fn pause(&mut self) {
        if self.state == PlaybackState::Playing {
            tracing::info!("pausing playback");
            self.state = PlaybackState::Paused;
            // TODO: Pause rodio sink
        }
    }

    /// Stop playback entirely.
    ///
    /// Resets the playback position. The queue and current index are preserved.
    pub fn stop(&mut self) {
        if self.state != PlaybackState::Stopped {
            tracing::info!("stopping playback");
            self.state = PlaybackState::Stopped;
            // TODO: Stop and drop rodio sink
        }
    }

    /// Advance to the next track in the queue.
    ///
    /// If already at the end of the queue, stops playback.
    pub fn next(&mut self) -> Result<()> {
        let Some(current) = self.current_index else {
            return Err(AudioError::Playback("no current track".into()));
        };

        let next_index = current + 1;
        if next_index >= self.queue.len() {
            tracing::info!("reached end of queue");
            self.stop();
            return Ok(());
        }

        tracing::info!(
            track = next_index,
            title = %self.queue[next_index].title,
            "advancing to next track"
        );

        self.current_index = Some(next_index);

        if self.state == PlaybackState::Playing {
            // Stop current, start next
            self.state = PlaybackState::Stopped;
            self.play()?;
        }

        Ok(())
    }

    /// Go back to the previous track in the queue.
    ///
    /// If already at the beginning, stays on the first track.
    pub fn previous(&mut self) -> Result<()> {
        let Some(current) = self.current_index else {
            return Err(AudioError::Playback("no current track".into()));
        };

        if current == 0 {
            tracing::info!("already at the beginning of queue");
            return Ok(());
        }

        let prev_index = current - 1;
        tracing::info!(
            track = prev_index,
            title = %self.queue[prev_index].title,
            "going to previous track"
        );

        self.current_index = Some(prev_index);

        if self.state == PlaybackState::Playing {
            self.state = PlaybackState::Stopped;
            self.play()?;
        }

        Ok(())
    }

    /// Set the playback volume.
    ///
    /// Volume is clamped to the range `[0.0, 1.0]`.
    pub fn set_volume(&mut self, vol: f32) {
        self.volume = vol.clamp(0.0, 1.0);
        tracing::debug!(volume = self.volume, "volume changed");
        // TODO: Apply volume to rodio sink
    }

    /// Add a track to the end of the queue.
    pub fn enqueue(&mut self, track: Track) {
        tracing::debug!(title = %track.title, "enqueuing track");
        self.queue.push(track);
    }

    /// Clear the entire queue and stop playback.
    pub fn clear_queue(&mut self) {
        tracing::info!("clearing queue ({} tracks)", self.queue.len());
        self.stop();
        self.queue.clear();
        self.current_index = None;
    }

    /// Get a reference to the currently selected track, if any.
    #[must_use]
    pub fn current_track(&self) -> Option<&Track> {
        self.current_index.map(|i| &self.queue[i])
    }

    /// Get the current playback state.
    #[must_use]
    pub fn state(&self) -> PlaybackState {
        self.state
    }

    /// Get a slice of all tracks in the queue.
    #[must_use]
    pub fn queue(&self) -> &[Track] {
        &self.queue
    }
}

/// Detect the codec name from a file extension.
///
/// Returns `None` for unrecognised extensions.
#[must_use]
pub fn detect_codec(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "flac" => Some("flac"),
        "alac" | "m4a" => Some("alac"),
        "wav" => Some("wav"),
        "aiff" | "aif" => Some("aiff"),
        "ogg" | "oga" => Some("vorbis"),
        "mp3" => Some("mp3"),
        "aac" => Some("aac"),
        "opus" => Some("opus"),
        "wma" => Some("wma"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> AudioConfig {
        AudioConfig {
            sample_rate: 44100,
            buffer_size: 4096,
            gapless: true,
        }
    }

    fn test_track(title: &str) -> Track {
        Track {
            path: PathBuf::from(format!("/tmp/test_{title}.flac")),
            title: title.to_string(),
            artist: Some("Test Artist".to_string()),
            album: Some("Test Album".to_string()),
            duration: Some(Duration::from_secs(180)),
            track_number: Some(1),
            codec: "flac".to_string(),
            sample_rate: Some(44100),
            bit_depth: Some(16),
        }
    }

    #[test]
    fn new_engine_starts_stopped() {
        let engine = AudioEngine::new(&test_config());
        assert_eq!(engine.state(), PlaybackState::Stopped);
        assert!(engine.queue().is_empty());
        assert!(engine.current_track().is_none());
    }

    #[test]
    fn play_empty_queue_returns_error() {
        let mut engine = AudioEngine::new(&test_config());
        let result = engine.play();
        assert!(result.is_err());
    }

    #[test]
    fn enqueue_adds_tracks() {
        let mut engine = AudioEngine::new(&test_config());
        engine.enqueue(test_track("a"));
        engine.enqueue(test_track("b"));
        assert_eq!(engine.queue().len(), 2);
        assert_eq!(engine.queue()[0].title, "a");
        assert_eq!(engine.queue()[1].title, "b");
    }

    #[test]
    fn clear_queue_empties_and_stops() {
        let mut engine = AudioEngine::new(&test_config());
        engine.enqueue(test_track("a"));
        engine.enqueue(test_track("b"));
        engine.clear_queue();
        assert!(engine.queue().is_empty());
        assert!(engine.current_track().is_none());
        assert_eq!(engine.state(), PlaybackState::Stopped);
    }

    #[test]
    fn pause_from_stopped_is_noop() {
        let mut engine = AudioEngine::new(&test_config());
        engine.pause();
        assert_eq!(engine.state(), PlaybackState::Stopped);
    }

    #[test]
    fn stop_from_stopped_is_noop() {
        let mut engine = AudioEngine::new(&test_config());
        engine.stop();
        assert_eq!(engine.state(), PlaybackState::Stopped);
    }

    #[test]
    fn set_volume_clamps_values() {
        let mut engine = AudioEngine::new(&test_config());
        engine.set_volume(1.5);
        assert!((engine.volume - 1.0).abs() < f32::EPSILON);

        engine.set_volume(-0.5);
        assert!(engine.volume.abs() < f32::EPSILON);

        engine.set_volume(0.5);
        assert!((engine.volume - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn next_without_current_returns_error() {
        let mut engine = AudioEngine::new(&test_config());
        engine.enqueue(test_track("a"));
        let result = engine.next();
        assert!(result.is_err());
    }

    #[test]
    fn previous_without_current_returns_error() {
        let mut engine = AudioEngine::new(&test_config());
        engine.enqueue(test_track("a"));
        let result = engine.previous();
        assert!(result.is_err());
    }

    #[test]
    fn next_at_end_of_queue_stops() {
        let mut engine = AudioEngine::new(&test_config());
        engine.enqueue(test_track("a"));
        engine.current_index = Some(0);
        engine.state = PlaybackState::Paused;
        let result = engine.next();
        assert!(result.is_ok());
        assert_eq!(engine.state(), PlaybackState::Stopped);
    }

    #[test]
    fn next_advances_index() {
        let mut engine = AudioEngine::new(&test_config());
        engine.enqueue(test_track("a"));
        engine.enqueue(test_track("b"));
        engine.enqueue(test_track("c"));
        engine.current_index = Some(0);
        engine.state = PlaybackState::Paused;

        let _ = engine.next();
        assert_eq!(engine.current_index, Some(1));
        assert_eq!(engine.current_track().unwrap().title, "b");
    }

    #[test]
    fn previous_at_beginning_stays() {
        let mut engine = AudioEngine::new(&test_config());
        engine.enqueue(test_track("a"));
        engine.enqueue(test_track("b"));
        engine.current_index = Some(0);
        engine.state = PlaybackState::Paused;

        let result = engine.previous();
        assert!(result.is_ok());
        assert_eq!(engine.current_index, Some(0));
    }

    #[test]
    fn previous_decrements_index() {
        let mut engine = AudioEngine::new(&test_config());
        engine.enqueue(test_track("a"));
        engine.enqueue(test_track("b"));
        engine.enqueue(test_track("c"));
        engine.current_index = Some(2);
        engine.state = PlaybackState::Paused;

        let _ = engine.previous();
        assert_eq!(engine.current_index, Some(1));
        assert_eq!(engine.current_track().unwrap().title, "b");
    }

    #[test]
    fn detect_codec_known_extensions() {
        assert_eq!(detect_codec(Path::new("song.flac")), Some("flac"));
        assert_eq!(detect_codec(Path::new("song.mp3")), Some("mp3"));
        assert_eq!(detect_codec(Path::new("song.wav")), Some("wav"));
        assert_eq!(detect_codec(Path::new("song.ogg")), Some("vorbis"));
        assert_eq!(detect_codec(Path::new("song.aiff")), Some("aiff"));
        assert_eq!(detect_codec(Path::new("song.m4a")), Some("alac"));
        assert_eq!(detect_codec(Path::new("song.opus")), Some("opus"));
        assert_eq!(detect_codec(Path::new("song.aac")), Some("aac"));
    }

    #[test]
    fn detect_codec_unknown_extension() {
        assert_eq!(detect_codec(Path::new("document.pdf")), None);
        assert_eq!(detect_codec(Path::new("noextension")), None);
    }

    #[test]
    fn detect_codec_case_insensitive() {
        assert_eq!(detect_codec(Path::new("SONG.FLAC")), Some("flac"));
        assert_eq!(detect_codec(Path::new("Song.Mp3")), Some("mp3"));
    }

    #[test]
    fn state_transitions_pause_resume() {
        let mut engine = AudioEngine::new(&test_config());
        engine.enqueue(test_track("a"));
        engine.current_index = Some(0);

        // Simulate playing state (can't call play() without a real file)
        engine.state = PlaybackState::Playing;
        assert_eq!(engine.state(), PlaybackState::Playing);

        engine.pause();
        assert_eq!(engine.state(), PlaybackState::Paused);

        // Resume via play() from paused
        let result = engine.play();
        assert!(result.is_ok());
        assert_eq!(engine.state(), PlaybackState::Playing);

        engine.stop();
        assert_eq!(engine.state(), PlaybackState::Stopped);
    }
}
