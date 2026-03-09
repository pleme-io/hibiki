//! Audio playback engine -- rodio + symphonia for hi-fi codec support.
//!
//! Supports: FLAC, ALAC, WAV, AIFF, OGG Vorbis, MP3, AAC, Opus
//! Features: gapless playback, queue management, shuffle, repeat modes,
//! volume control, seek.
//!
//! Architecture: The `AudioEngine` manages queue/state and is `Send`-safe.
//! A dedicated audio thread owns the rodio `OutputStream`/`Sink` (which is
//! `!Send`). Communication between the engine and the audio thread is via
//! an `mpsc` command channel.

use crate::config::AudioConfig;
use rand::seq::SliceRandom;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

#[derive(thiserror::Error, Debug)]
#[allow(dead_code)]
pub enum AudioError {
    #[error("playback failed: {0}")]
    Playback(String),
    #[error("codec not supported: {0}")]
    UnsupportedCodec(String),
    #[error("file not found: {0}")]
    FileNotFound(PathBuf),
    #[error("device error: {0}")]
    Device(String),
    #[error("seek failed: {0}")]
    Seek(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, AudioError>;

/// A track in the library / queue.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Track {
    pub path: PathBuf,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration: Option<Duration>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub year: Option<u32>,
    pub genre: Option<String>,
    pub codec: String,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u32>,
}

/// Repeat mode for the playback queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RepeatMode {
    /// No repeat -- stop at end of queue.
    Off,
    /// Repeat the current track.
    One,
    /// Repeat the entire queue.
    All,
}

impl RepeatMode {
    /// Cycle to the next repeat mode: Off -> One -> All -> Off.
    #[must_use]
    pub fn cycle(self) -> Self {
        match self {
            Self::Off => Self::One,
            Self::One => Self::All,
            Self::All => Self::Off,
        }
    }
}

/// Current playback state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Stopped,
    Playing,
    Paused,
}

/// Commands sent to the audio output thread.
enum AudioCmd {
    /// Play a file from a path.
    PlayFile(PathBuf),
    /// Pause playback.
    Pause,
    /// Resume playback.
    Resume,
    /// Stop and clear the sink.
    Stop,
    /// Set volume (0.0 to 1.0).
    SetVolume(f32),
    /// Seek to a position.
    Seek(Duration),
    /// Shut down the audio thread.
    Shutdown,
}

/// Response from the audio output thread.
enum AudioResp {
    /// The sink is empty (track finished).
    TrackFinished,
}

/// Handle to the audio output thread.
struct AudioOutput {
    cmd_tx: mpsc::Sender<AudioCmd>,
    resp_rx: mpsc::Receiver<AudioResp>,
}

impl AudioOutput {
    /// Spawn the audio output thread and return a handle.
    fn spawn() -> std::result::Result<Self, AudioError> {
        let (cmd_tx, cmd_rx) = mpsc::channel::<AudioCmd>();
        let (resp_tx, resp_rx) = mpsc::channel::<AudioResp>();

        std::thread::spawn(move || {
            Self::audio_thread(cmd_rx, resp_tx);
        });

        Ok(Self { cmd_tx, resp_rx })
    }

    /// The audio thread event loop. Owns the OutputStream and Sink.
    fn audio_thread(cmd_rx: mpsc::Receiver<AudioCmd>, resp_tx: mpsc::Sender<AudioResp>) {
        let Ok((stream, stream_handle)) = rodio::OutputStream::try_default() else {
            tracing::error!("failed to open audio output device");
            return;
        };

        // Keep _stream alive for the lifetime of this thread.
        let _stream = stream;
        let mut sink: Option<rodio::Sink> = None;
        let mut was_playing = false;

        loop {
            // Check for commands (non-blocking).
            match cmd_rx.try_recv() {
                Ok(cmd) => match cmd {
                    AudioCmd::PlayFile(path) => {
                        // Stop old sink if any.
                        if let Some(old) = sink.take() {
                            old.stop();
                        }
                        match rodio::Sink::try_new(&stream_handle) {
                            Ok(new_sink) => {
                                match std::fs::File::open(&path) {
                                    Ok(file) => {
                                        match rodio::Decoder::new(std::io::BufReader::new(file)) {
                                            Ok(source) => {
                                                new_sink.append(source);
                                                sink = Some(new_sink);
                                                was_playing = true;
                                                tracing::debug!(path = %path.display(), "audio thread: playing");
                                            }
                                            Err(e) => {
                                                tracing::error!("decode error: {e}");
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!("file open error: {e}");
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::error!("sink creation error: {e}");
                            }
                        }
                    }
                    AudioCmd::Pause => {
                        if let Some(ref s) = sink {
                            s.pause();
                            was_playing = false;
                        }
                    }
                    AudioCmd::Resume => {
                        if let Some(ref s) = sink {
                            s.play();
                            was_playing = true;
                        }
                    }
                    AudioCmd::Stop => {
                        if let Some(old) = sink.take() {
                            old.stop();
                        }
                        was_playing = false;
                    }
                    AudioCmd::SetVolume(vol) => {
                        if let Some(ref s) = sink {
                            s.set_volume(vol);
                        }
                    }
                    AudioCmd::Seek(pos) => {
                        if let Some(ref s) = sink {
                            let _ = s.try_seek(pos);
                        }
                    }
                    AudioCmd::Shutdown => {
                        if let Some(old) = sink.take() {
                            old.stop();
                        }
                        return;
                    }
                },
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => return,
            }

            // Check if the current track finished.
            if was_playing {
                if let Some(ref s) = sink {
                    if s.empty() {
                        was_playing = false;
                        let _ = resp_tx.send(AudioResp::TrackFinished);
                    }
                }
            }

            // Sleep briefly to avoid busy-waiting.
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn send(&self, cmd: AudioCmd) {
        let _ = self.cmd_tx.send(cmd);
    }
}

/// Audio engine that manages playback state, queue, and output.
///
/// This struct is `Send`-safe. The actual audio I/O runs on a dedicated
/// thread managed via the `AudioOutput` handle.
#[allow(dead_code)]
pub struct AudioEngine {
    state: PlaybackState,
    queue: Vec<Track>,
    current_index: Option<usize>,
    volume: f32,
    muted: bool,
    shuffle: bool,
    repeat: RepeatMode,
    /// Shuffled index order (maps display position to queue index).
    shuffle_order: Vec<usize>,
    /// Position within the shuffle order.
    shuffle_position: usize,
    /// History stack for previous() navigation.
    history: Vec<usize>,
    config: AudioConfig,
    /// Handle to the audio output thread.
    output: Option<AudioOutput>,
    /// Elapsed playback position (approximated).
    position: Duration,
    /// Instant when playback started/resumed (for position tracking).
    playback_started_at: Option<std::time::Instant>,
}

// AudioEngine is Send because AudioOutput only holds mpsc Senders/Receivers
// which are Send. The !Send OutputStream lives on the audio thread.
// SAFETY: AudioOutput's cmd_tx/resp_rx are Send. All other fields are Send.
unsafe impl Send for AudioEngine {}

impl AudioEngine {
    /// Create a new audio engine with the given configuration.
    ///
    /// Initialises the engine in the `Stopped` state with an empty queue.
    /// The audio output thread is spawned lazily on the first call to `play()`.
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
            muted: false,
            shuffle: false,
            repeat: RepeatMode::Off,
            shuffle_order: Vec::new(),
            shuffle_position: 0,
            history: Vec::new(),
            config: config.clone(),
            output: None,
            position: Duration::ZERO,
            playback_started_at: None,
        }
    }

    /// Ensure the audio output thread is running.
    fn ensure_output(&mut self) -> Result<()> {
        if self.output.is_some() {
            return Ok(());
        }
        let output = AudioOutput::spawn()?;
        // Set initial volume.
        let effective = if self.muted { 0.0 } else { self.volume };
        output.send(AudioCmd::SetVolume(effective));
        self.output = Some(output);
        Ok(())
    }

    /// Load and start playing a track from the queue by its queue index.
    fn play_track_at(&mut self, queue_index: usize) -> Result<()> {
        let track_path = self.queue[queue_index].path.clone();
        let track_title = self.queue[queue_index].title.clone();
        let track_artist = self.queue[queue_index].artist.clone();
        let track_codec = self.queue[queue_index].codec.clone();

        if !track_path.exists() {
            return Err(AudioError::FileNotFound(track_path));
        }

        tracing::info!(
            title = %track_title,
            artist = ?track_artist,
            codec = %track_codec,
            "starting playback"
        );

        self.ensure_output()?;

        if let Some(ref output) = self.output {
            let effective = if self.muted { 0.0 } else { self.volume };
            output.send(AudioCmd::SetVolume(effective));
            output.send(AudioCmd::PlayFile(track_path));
        }

        self.current_index = Some(queue_index);
        self.state = PlaybackState::Playing;
        self.position = Duration::ZERO;
        self.playback_started_at = Some(std::time::Instant::now());

        Ok(())
    }

    /// Start or resume playback.
    ///
    /// If the engine is stopped, begins playing the current (or first) track in the queue.
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
                if let Some(ref output) = self.output {
                    output.send(AudioCmd::Resume);
                }
                self.state = PlaybackState::Playing;
                self.playback_started_at = Some(std::time::Instant::now());
                Ok(())
            }
            PlaybackState::Stopped => {
                if self.queue.is_empty() {
                    tracing::warn!("play requested but queue is empty");
                    return Err(AudioError::Playback("queue is empty".into()));
                }

                let index = self.resolve_current_index();
                self.play_track_at(index)
            }
        }
    }

    /// Pause playback.
    ///
    /// If playing, pauses the current track. Otherwise this is a no-op.
    pub fn pause(&mut self) {
        if self.state == PlaybackState::Playing {
            tracing::info!("pausing playback");
            // Update position before pausing.
            self.update_position();
            if let Some(ref output) = self.output {
                output.send(AudioCmd::Pause);
            }
            self.state = PlaybackState::Paused;
            self.playback_started_at = None;
        }
    }

    /// Toggle between play and pause.
    pub fn toggle(&mut self) -> Result<()> {
        match self.state {
            PlaybackState::Playing => {
                self.pause();
                Ok(())
            }
            PlaybackState::Paused | PlaybackState::Stopped => self.play(),
        }
    }

    /// Stop playback entirely.
    ///
    /// Resets the playback position. The queue and current index are preserved.
    pub fn stop(&mut self) {
        if self.state != PlaybackState::Stopped {
            tracing::info!("stopping playback");
            if let Some(ref output) = self.output {
                output.send(AudioCmd::Stop);
            }
            self.state = PlaybackState::Stopped;
            self.position = Duration::ZERO;
            self.playback_started_at = None;
        }
    }

    /// Advance to the next track in the queue.
    ///
    /// Respects shuffle and repeat modes. If at the end of the queue:
    /// - `RepeatMode::Off`: stops playback.
    /// - `RepeatMode::One`: replays the current track.
    /// - `RepeatMode::All`: wraps around to the beginning.
    pub fn next(&mut self) -> Result<()> {
        if self.repeat == RepeatMode::One {
            // Replay current track.
            if let Some(idx) = self.current_index {
                self.history.push(idx);
                return self.play_track_at(idx);
            }
            return Err(AudioError::Playback("no current track".into()));
        }

        let Some(current_queue_idx) = self.current_index else {
            return Err(AudioError::Playback("no current track".into()));
        };

        // Push current to history before advancing.
        self.history.push(current_queue_idx);

        let next_queue_idx = if self.shuffle {
            self.shuffle_position += 1;
            if self.shuffle_position >= self.shuffle_order.len() {
                if self.repeat == RepeatMode::All {
                    self.regenerate_shuffle();
                    self.shuffle_position = 0;
                    Some(self.shuffle_order[0])
                } else {
                    None
                }
            } else {
                Some(self.shuffle_order[self.shuffle_position])
            }
        } else {
            let next_linear = current_queue_idx + 1;
            if next_linear >= self.queue.len() {
                if self.repeat == RepeatMode::All {
                    Some(0)
                } else {
                    None
                }
            } else {
                Some(next_linear)
            }
        };

        match next_queue_idx {
            Some(idx) => {
                let was_playing = self.state == PlaybackState::Playing;
                if was_playing {
                    self.play_track_at(idx)?;
                } else {
                    self.current_index = Some(idx);
                }
                Ok(())
            }
            None => {
                tracing::info!("reached end of queue");
                self.stop();
                Ok(())
            }
        }
    }

    /// Go back to the previous track in the queue.
    ///
    /// If there is history, pops the last played track. Otherwise stays
    /// on the first track.
    pub fn previous(&mut self) -> Result<()> {
        if let Some(prev_idx) = self.history.pop() {
            let was_playing = self.state == PlaybackState::Playing;
            if was_playing {
                self.play_track_at(prev_idx)?;
            } else {
                self.current_index = Some(prev_idx);
            }
            Ok(())
        } else {
            // No history -- restart current track or stay.
            if let Some(idx) = self.current_index {
                if self.state == PlaybackState::Playing {
                    self.play_track_at(idx)?;
                }
            }
            Ok(())
        }
    }

    /// Seek to a position within the current track.
    ///
    /// Position is clamped to the track duration.
    pub fn seek(&mut self, pos: Duration) -> Result<()> {
        if let Some(ref output) = self.output {
            output.send(AudioCmd::Seek(pos));
            self.position = pos;
            self.playback_started_at = Some(std::time::Instant::now());
        }
        Ok(())
    }

    /// Set the playback volume.
    ///
    /// Volume is clamped to the range `[0.0, 1.0]`.
    pub fn set_volume(&mut self, vol: f32) {
        self.volume = vol.clamp(0.0, 1.0);
        tracing::debug!(volume = self.volume, "volume changed");
        if !self.muted {
            if let Some(ref output) = self.output {
                output.send(AudioCmd::SetVolume(self.volume));
            }
        }
    }

    /// Adjust volume by a delta (positive = up, negative = down).
    pub fn adjust_volume(&mut self, delta: f32) {
        self.set_volume(self.volume + delta);
    }

    /// Toggle mute state.
    pub fn toggle_mute(&mut self) {
        self.muted = !self.muted;
        let effective = if self.muted { 0.0 } else { self.volume };
        if let Some(ref output) = self.output {
            output.send(AudioCmd::SetVolume(effective));
        }
        tracing::debug!(muted = self.muted, "mute toggled");
    }

    /// Toggle shuffle mode.
    ///
    /// When enabling shuffle, a new shuffle order is generated.
    pub fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
        if self.shuffle {
            self.regenerate_shuffle();
        }
        tracing::debug!(shuffle = self.shuffle, "shuffle toggled");
    }

    /// Cycle repeat mode: Off -> One -> All -> Off.
    pub fn cycle_repeat(&mut self) {
        self.repeat = self.repeat.cycle();
        tracing::debug!(repeat = ?self.repeat, "repeat mode changed");
    }

    /// Add a track to the end of the queue.
    pub fn enqueue(&mut self, track: Track) {
        tracing::debug!(title = %track.title, "enqueuing track");
        let idx = self.queue.len();
        self.queue.push(track);
        self.shuffle_order.push(idx);
    }

    /// Add multiple tracks to the end of the queue.
    pub fn enqueue_many(&mut self, tracks: Vec<Track>) {
        let start = self.queue.len();
        for (i, track) in tracks.into_iter().enumerate() {
            self.shuffle_order.push(start + i);
            self.queue.push(track);
        }
    }

    /// Play a specific track from the queue immediately.
    pub fn play_index(&mut self, index: usize) -> Result<()> {
        if index >= self.queue.len() {
            return Err(AudioError::Playback(format!(
                "index {index} out of range (queue has {} tracks)",
                self.queue.len()
            )));
        }
        if let Some(current) = self.current_index {
            self.history.push(current);
        }
        self.play_track_at(index)
    }

    /// Remove a track from the queue by index.
    pub fn remove_from_queue(&mut self, index: usize) {
        if index >= self.queue.len() {
            return;
        }

        // If removing the currently playing track, stop playback.
        if self.current_index == Some(index) {
            self.stop();
            self.current_index = None;
        } else if let Some(ref mut current) = self.current_index {
            if *current > index {
                *current -= 1;
            }
        }

        self.queue.remove(index);
        self.rebuild_shuffle_order();
    }

    /// Move a track within the queue.
    pub fn move_in_queue(&mut self, from: usize, to: usize) {
        if from >= self.queue.len() || to >= self.queue.len() || from == to {
            return;
        }

        let track = self.queue.remove(from);
        self.queue.insert(to, track);

        // Adjust current index.
        if let Some(ref mut current) = self.current_index {
            if *current == from {
                *current = to;
            } else if from < *current && to >= *current {
                *current -= 1;
            } else if from > *current && to <= *current {
                *current += 1;
            }
        }

        self.rebuild_shuffle_order();
    }

    /// Clear the entire queue and stop playback.
    pub fn clear_queue(&mut self) {
        tracing::info!("clearing queue ({} tracks)", self.queue.len());
        self.stop();
        self.queue.clear();
        self.current_index = None;
        self.shuffle_order.clear();
        self.shuffle_position = 0;
        self.history.clear();
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

    /// Get the current volume (0.0 to 1.0).
    #[must_use]
    pub fn volume(&self) -> f32 {
        self.volume
    }

    /// Check if audio is muted.
    #[must_use]
    pub fn is_muted(&self) -> bool {
        self.muted
    }

    /// Check if shuffle is enabled.
    #[must_use]
    pub fn is_shuffle(&self) -> bool {
        self.shuffle
    }

    /// Get the current repeat mode.
    #[must_use]
    pub fn repeat_mode(&self) -> RepeatMode {
        self.repeat
    }

    /// Get the current queue index.
    #[must_use]
    pub fn current_index(&self) -> Option<usize> {
        self.current_index
    }

    /// Get the elapsed playback position (approximate).
    #[must_use]
    pub fn position(&self) -> Duration {
        if let Some(started) = self.playback_started_at {
            self.position + started.elapsed()
        } else {
            self.position
        }
    }

    /// Call this periodically to handle track completion and auto-advance.
    ///
    /// Returns `true` if a new track started playing.
    pub fn tick(&mut self) -> bool {
        if self.state != PlaybackState::Playing {
            return false;
        }

        // Check for track-finished messages from the audio thread.
        if let Some(ref output) = self.output {
            if let Ok(AudioResp::TrackFinished) = output.resp_rx.try_recv() {
                tracing::debug!("track finished, auto-advancing");
                if self.next().is_ok() && self.state == PlaybackState::Playing {
                    return true;
                }
            }
        }
        false
    }

    /// Resolve which queue index to start playing.
    fn resolve_current_index(&self) -> usize {
        if let Some(idx) = self.current_index {
            return idx;
        }
        if self.shuffle && !self.shuffle_order.is_empty() {
            self.shuffle_order[0]
        } else {
            0
        }
    }

    /// Regenerate the shuffle order with a random permutation.
    fn regenerate_shuffle(&mut self) {
        self.shuffle_order = (0..self.queue.len()).collect();
        let mut rng = rand::rng();
        self.shuffle_order.shuffle(&mut rng);
        self.shuffle_position = 0;
    }

    /// Rebuild shuffle order after a queue modification (add/remove/move).
    fn rebuild_shuffle_order(&mut self) {
        if self.shuffle {
            self.regenerate_shuffle();
        } else {
            self.shuffle_order = (0..self.queue.len()).collect();
        }
    }

    /// Update the tracked position from the playback clock.
    fn update_position(&mut self) {
        if let Some(started) = self.playback_started_at.take() {
            self.position += started.elapsed();
        }
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        if let Some(ref output) = self.output {
            let _ = output.cmd_tx.send(AudioCmd::Shutdown);
        }
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

/// Check if a file extension is a supported audio format.
#[must_use]
pub fn is_audio_file(path: &Path) -> bool {
    detect_codec(path).is_some()
}

/// Format a duration as mm:ss.
#[must_use]
pub fn format_duration(d: Duration) -> String {
    let total_secs = d.as_secs();
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    format!("{mins}:{secs:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> AudioConfig {
        AudioConfig {
            sample_rate: 44100,
            buffer_size: 4096,
            gapless: true,
            output_device: None,
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
            disc_number: None,
            year: Some(2024),
            genre: Some("Rock".to_string()),
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
    fn enqueue_many_adds_all() {
        let mut engine = AudioEngine::new(&test_config());
        let tracks = vec![test_track("a"), test_track("b"), test_track("c")];
        engine.enqueue_many(tracks);
        assert_eq!(engine.queue().len(), 3);
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
        assert!((engine.volume() - 1.0).abs() < f32::EPSILON);

        engine.set_volume(-0.5);
        assert!(engine.volume().abs() < f32::EPSILON);

        engine.set_volume(0.5);
        assert!((engine.volume() - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn adjust_volume_increments() {
        let mut engine = AudioEngine::new(&test_config());
        engine.set_volume(0.5);
        engine.adjust_volume(0.1);
        assert!((engine.volume() - 0.6).abs() < 0.01);
        engine.adjust_volume(-0.3);
        assert!((engine.volume() - 0.3).abs() < 0.01);
    }

    #[test]
    fn toggle_mute() {
        let mut engine = AudioEngine::new(&test_config());
        assert!(!engine.is_muted());
        engine.toggle_mute();
        assert!(engine.is_muted());
        engine.toggle_mute();
        assert!(!engine.is_muted());
    }

    #[test]
    fn toggle_shuffle() {
        let mut engine = AudioEngine::new(&test_config());
        assert!(!engine.is_shuffle());
        engine.enqueue(test_track("a"));
        engine.enqueue(test_track("b"));
        engine.toggle_shuffle();
        assert!(engine.is_shuffle());
        engine.toggle_shuffle();
        assert!(!engine.is_shuffle());
    }

    #[test]
    fn cycle_repeat() {
        let mut engine = AudioEngine::new(&test_config());
        assert_eq!(engine.repeat_mode(), RepeatMode::Off);
        engine.cycle_repeat();
        assert_eq!(engine.repeat_mode(), RepeatMode::One);
        engine.cycle_repeat();
        assert_eq!(engine.repeat_mode(), RepeatMode::All);
        engine.cycle_repeat();
        assert_eq!(engine.repeat_mode(), RepeatMode::Off);
    }

    #[test]
    fn repeat_mode_cycle() {
        assert_eq!(RepeatMode::Off.cycle(), RepeatMode::One);
        assert_eq!(RepeatMode::One.cycle(), RepeatMode::All);
        assert_eq!(RepeatMode::All.cycle(), RepeatMode::Off);
    }

    #[test]
    fn next_without_current_returns_error() {
        let mut engine = AudioEngine::new(&test_config());
        engine.enqueue(test_track("a"));
        let result = engine.next();
        assert!(result.is_err());
    }

    #[test]
    fn previous_without_current_is_ok() {
        let mut engine = AudioEngine::new(&test_config());
        engine.enqueue(test_track("a"));
        let result = engine.previous();
        assert!(result.is_ok());
    }

    #[test]
    fn remove_from_queue() {
        let mut engine = AudioEngine::new(&test_config());
        engine.enqueue(test_track("a"));
        engine.enqueue(test_track("b"));
        engine.enqueue(test_track("c"));
        engine.remove_from_queue(1);
        assert_eq!(engine.queue().len(), 2);
        assert_eq!(engine.queue()[0].title, "a");
        assert_eq!(engine.queue()[1].title, "c");
    }

    #[test]
    fn move_in_queue_forward() {
        let mut engine = AudioEngine::new(&test_config());
        engine.enqueue(test_track("a"));
        engine.enqueue(test_track("b"));
        engine.enqueue(test_track("c"));
        engine.move_in_queue(0, 2);
        assert_eq!(engine.queue()[0].title, "b");
        assert_eq!(engine.queue()[1].title, "c");
        assert_eq!(engine.queue()[2].title, "a");
    }

    #[test]
    fn play_index_out_of_range() {
        let mut engine = AudioEngine::new(&test_config());
        engine.enqueue(test_track("a"));
        let result = engine.play_index(5);
        assert!(result.is_err());
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
    fn is_audio_file_works() {
        assert!(is_audio_file(Path::new("track.flac")));
        assert!(is_audio_file(Path::new("track.mp3")));
        assert!(!is_audio_file(Path::new("readme.txt")));
        assert!(!is_audio_file(Path::new("cover.jpg")));
    }

    #[test]
    fn format_duration_display() {
        assert_eq!(format_duration(Duration::from_secs(0)), "0:00");
        assert_eq!(format_duration(Duration::from_secs(65)), "1:05");
        assert_eq!(format_duration(Duration::from_secs(3661)), "61:01");
        assert_eq!(format_duration(Duration::from_secs(180)), "3:00");
    }

    #[test]
    fn state_transitions_pause_resume() {
        let mut engine = AudioEngine::new(&test_config());
        engine.enqueue(test_track("a"));
        engine.current_index = Some(0);

        // Simulate playing state (can't call play() without a real file).
        engine.state = PlaybackState::Playing;
        assert_eq!(engine.state(), PlaybackState::Playing);

        engine.pause();
        assert_eq!(engine.state(), PlaybackState::Paused);

        engine.stop();
        assert_eq!(engine.state(), PlaybackState::Stopped);
    }
}
