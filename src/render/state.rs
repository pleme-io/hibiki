//! UI state for the hibiki player -- tracks which panel is focused,
//! search query, command buffer, and widget states.

use crate::audio::{AudioEngine, PlaybackState, RepeatMode, Track};
use crate::input::InputMode;
use egaku::{FocusManager, ListView, TabBar, TextInput};

/// Panel identifiers for the three-column layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Library,
    Player,
    Queue,
    Torrent,
}

impl Panel {
    /// Panel names for display.
    #[must_use]
    #[allow(dead_code)]
    pub fn label(self) -> &'static str {
        match self {
            Self::Library => "Library",
            Self::Player => "Player",
            Self::Queue => "Queue",
            Self::Torrent => "Torrents",
        }
    }

    /// Index for focus manager / tab bar.
    #[must_use]
    pub fn index(self) -> usize {
        match self {
            Self::Library => 0,
            Self::Player => 1,
            Self::Queue => 2,
            Self::Torrent => 3,
        }
    }

    /// From index.
    #[must_use]
    pub fn from_index(idx: usize) -> Self {
        match idx {
            0 => Self::Library,
            1 => Self::Player,
            2 => Self::Queue,
            3 => Self::Torrent,
            _ => Self::Library,
        }
    }
}

/// The complete UI state, owned by the renderer.
pub struct UiState {
    /// Current input mode.
    pub mode: InputMode,
    /// Active panel.
    pub active_panel: Panel,
    /// Tab bar for panel switching.
    pub tabs: TabBar,
    /// Focus manager.
    pub focus: FocusManager,
    /// Library track list widget.
    pub library_list: ListView,
    /// Queue track list widget.
    pub queue_list: ListView,
    /// Search input widget.
    pub search_input: TextInput,
    /// Command input widget.
    pub command_input: TextInput,
    /// Current search query (for filtering).
    pub search_query: String,
    /// Status message (shown in the bottom bar).
    #[allow(dead_code)]
    pub status_message: Option<String>,
    /// Window dimensions.
    pub width: u32,
    pub height: u32,
}

impl UiState {
    /// Create a new UI state with default widget configurations.
    #[must_use]
    pub fn new() -> Self {
        let tabs = TabBar::new(vec![
            "Library".into(),
            "Player".into(),
            "Queue".into(),
            "Torrents".into(),
        ]);

        let focus = FocusManager::new(vec![
            "library".into(),
            "player".into(),
            "queue".into(),
            "torrents".into(),
        ]);

        Self {
            mode: InputMode::Normal,
            active_panel: Panel::Library,
            tabs,
            focus,
            library_list: ListView::new(Vec::new(), 20),
            queue_list: ListView::new(Vec::new(), 20),
            search_input: TextInput::new(),
            command_input: TextInput::new(),
            search_query: String::new(),
            status_message: None,
            width: 1280,
            height: 720,
        }
    }

    /// Update the library list from a set of tracks.
    pub fn update_library_list(&mut self, tracks: &[Track]) {
        let items: Vec<String> = tracks
            .iter()
            .map(|t| {
                let artist = t.artist.as_deref().unwrap_or("Unknown");
                format!("{} - {}", artist, t.title)
            })
            .collect();
        self.library_list.set_items(items);
    }

    /// Update the queue list from the audio engine's queue.
    pub fn update_queue_list(&mut self, engine: &AudioEngine) {
        let items: Vec<String> = engine
            .queue()
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let prefix = if engine.current_index() == Some(i) {
                    ">"
                } else {
                    " "
                };
                let artist = t.artist.as_deref().unwrap_or("Unknown");
                format!("{prefix} {artist} - {}", t.title)
            })
            .collect();
        self.queue_list.set_items(items);
    }

    /// Switch to a specific panel.
    pub fn switch_panel(&mut self, panel: Panel) {
        self.active_panel = panel;
        self.tabs.select(panel.index());
        self.focus.set_focus(match panel {
            Panel::Library => "library",
            Panel::Player => "player",
            Panel::Queue => "queue",
            Panel::Torrent => "torrents",
        });
        self.mode = match panel {
            Panel::Library => InputMode::Library,
            Panel::Queue => InputMode::Queue,
            Panel::Torrent => InputMode::Torrent,
            Panel::Player => InputMode::Normal,
        };
    }

    /// Advance to the next panel.
    pub fn next_panel(&mut self) {
        self.tabs.select_next();
        let panel = Panel::from_index(self.tabs.active_index());
        self.switch_panel(panel);
    }

    /// Go to the previous panel.
    pub fn prev_panel(&mut self) {
        self.tabs.select_prev();
        let panel = Panel::from_index(self.tabs.active_index());
        self.switch_panel(panel);
    }

    /// Format the status bar text from the current audio engine state.
    #[must_use]
    pub fn format_status_bar(&self, engine: &AudioEngine) -> String {
        let state_icon = match engine.state() {
            PlaybackState::Playing => "[>]",
            PlaybackState::Paused => "[||]",
            PlaybackState::Stopped => "[.]",
        };

        let track_info = engine
            .current_track()
            .map(|t| {
                let artist = t.artist.as_deref().unwrap_or("Unknown");
                let duration = t
                    .duration
                    .map(crate::audio::format_duration)
                    .unwrap_or_else(|| "--:--".into());
                let position = crate::audio::format_duration(engine.position());
                format!("{artist} - {} [{position}/{duration}]", t.title)
            })
            .unwrap_or_else(|| "No track".into());

        let volume = if engine.is_muted() {
            "MUTE".into()
        } else {
            format!("{}%", (engine.volume() * 100.0) as u32)
        };

        let shuffle = if engine.is_shuffle() { "S" } else { "-" };
        let repeat = match engine.repeat_mode() {
            RepeatMode::Off => "-",
            RepeatMode::One => "R1",
            RepeatMode::All => "RA",
        };

        format!("{state_icon} {track_info} | Vol: {volume} | {shuffle} {repeat}")
    }
}

impl Default for UiState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AudioConfig;

    #[test]
    fn new_ui_state_defaults() {
        let state = UiState::new();
        assert_eq!(state.mode, InputMode::Normal);
        assert_eq!(state.active_panel, Panel::Library);
        assert_eq!(state.tabs.active_index(), 0);
        assert!(state.library_list.is_empty());
        assert!(state.queue_list.is_empty());
        assert!(state.search_query.is_empty());
    }

    #[test]
    fn panel_labels() {
        assert_eq!(Panel::Library.label(), "Library");
        assert_eq!(Panel::Player.label(), "Player");
        assert_eq!(Panel::Queue.label(), "Queue");
        assert_eq!(Panel::Torrent.label(), "Torrents");
    }

    #[test]
    fn panel_roundtrip() {
        for i in 0..4 {
            let panel = Panel::from_index(i);
            assert_eq!(panel.index(), i);
        }
    }

    #[test]
    fn switch_panel() {
        let mut state = UiState::new();
        state.switch_panel(Panel::Queue);
        assert_eq!(state.active_panel, Panel::Queue);
        assert_eq!(state.tabs.active_index(), 2);
        assert_eq!(state.mode, InputMode::Queue);
    }

    #[test]
    fn next_panel_cycles() {
        let mut state = UiState::new();
        state.next_panel();
        assert_eq!(state.active_panel, Panel::Player);
        state.next_panel();
        assert_eq!(state.active_panel, Panel::Queue);
        state.next_panel();
        assert_eq!(state.active_panel, Panel::Torrent);
        state.next_panel();
        assert_eq!(state.active_panel, Panel::Library);
    }

    #[test]
    fn format_status_bar_no_track() {
        let state = UiState::new();
        let config = AudioConfig::default();
        let engine = AudioEngine::new(&config);
        let bar = state.format_status_bar(&engine);
        assert!(bar.contains("No track"));
        assert!(bar.contains("[.]"));
    }

    #[test]
    fn update_library_list() {
        let mut state = UiState::new();
        let tracks = vec![
            crate::audio::Track {
                path: "/a.flac".into(),
                title: "Song A".into(),
                artist: Some("Artist X".into()),
                album: None,
                duration: None,
                track_number: None,
                disc_number: None,
                year: None,
                genre: None,
                codec: "flac".into(),
                sample_rate: None,
                bit_depth: None,
            },
        ];
        state.update_library_list(&tracks);
        assert_eq!(state.library_list.len(), 1);
        assert!(state.library_list.selected_item().unwrap().contains("Song A"));
    }
}
