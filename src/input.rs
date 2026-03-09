//! Input handling -- vim-style keyboard navigation for the music player UI.
//!
//! Modal keybindings: Normal, Library, Queue, Search, Command modes.
//! Integrates with madori's `AppEvent::Key` events and uses awase types
//! for hotkey representation and parsing.

use madori::event::{KeyCode, KeyEvent};
#[cfg(test)]
use madori::event::Modifiers;

/// UI input mode (determines which keybindings are active).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    /// Default mode: global playback controls.
    Normal,
    /// Library browser focused: hjkl navigation, Enter to play.
    Library,
    /// Queue panel focused: hjkl navigation, d to remove.
    Queue,
    /// Search input active: typing filters the library.
    Search,
    /// Command input active: `:command` entry.
    Command,
    /// Torrent panel focused.
    Torrent,
}

/// Actions that can be triggered by keyboard input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    // -- Playback --
    TogglePlay,
    Stop,
    NextTrack,
    PrevTrack,
    VolumeUp,
    VolumeDown,
    ToggleMute,
    ToggleShuffle,
    CycleRepeat,
    SeekForward,
    SeekBackward,

    // -- Navigation --
    MoveUp,
    MoveDown,
    MoveLeft,
    MoveRight,
    PageUp,
    PageDown,
    GoToTop,
    GoToBottom,
    NextPanel,
    PrevPanel,
    JumpToPanel(usize),

    // -- Selection --
    Select,
    AddToQueue,
    RemoveFromQueue,
    MoveQueueItemUp,
    MoveQueueItemDown,
    ClearQueue,

    // -- Mode switching --
    EnterSearch,
    EnterCommand,
    ExitMode,
    Quit,

    // -- Search / command input --
    SearchChar(char),
    SearchBackspace,
    #[allow(dead_code)]
    SearchClear,
    SubmitSearch,
    SubmitCommand(String),
    CommandChar(char),
    CommandBackspace,

    // -- No action --
    None,
}

/// Convert a madori `KeyEvent` to an awase `Hotkey` (when possible).
///
/// This provides a bridge between madori's input events and awase's
/// hotkey system, enabling user-configurable keybindings via awase's
/// `Hotkey::parse()` format (e.g., `"cmd+space"`, `"ctrl+n"`).
#[must_use]
pub fn to_awase_hotkey(event: &KeyEvent) -> Option<awase::Hotkey> {
    let key = madori_key_to_awase(&event.key)?;
    let mut mods = awase::Modifiers::NONE;
    if event.modifiers.shift {
        mods |= awase::Modifiers::SHIFT;
    }
    if event.modifiers.ctrl {
        mods |= awase::Modifiers::CTRL;
    }
    if event.modifiers.alt {
        mods |= awase::Modifiers::ALT;
    }
    if event.modifiers.meta {
        mods |= awase::Modifiers::CMD;
    }
    Some(awase::Hotkey::new(mods, key))
}

/// Map a madori `KeyCode` to an awase `Key`.
fn madori_key_to_awase(key: &KeyCode) -> Option<awase::Key> {
    match key {
        KeyCode::Char(c) => match c.to_ascii_lowercase() {
            'a' => Some(awase::Key::A), 'b' => Some(awase::Key::B),
            'c' => Some(awase::Key::C), 'd' => Some(awase::Key::D),
            'e' => Some(awase::Key::E), 'f' => Some(awase::Key::F),
            'g' => Some(awase::Key::G), 'h' => Some(awase::Key::H),
            'i' => Some(awase::Key::I), 'j' => Some(awase::Key::J),
            'k' => Some(awase::Key::K), 'l' => Some(awase::Key::L),
            'm' => Some(awase::Key::M), 'n' => Some(awase::Key::N),
            'o' => Some(awase::Key::O), 'p' => Some(awase::Key::P),
            'q' => Some(awase::Key::Q), 'r' => Some(awase::Key::R),
            's' => Some(awase::Key::S), 't' => Some(awase::Key::T),
            'u' => Some(awase::Key::U), 'v' => Some(awase::Key::V),
            'w' => Some(awase::Key::W), 'x' => Some(awase::Key::X),
            'y' => Some(awase::Key::Y), 'z' => Some(awase::Key::Z),
            '0' => Some(awase::Key::Num0), '1' => Some(awase::Key::Num1),
            '2' => Some(awase::Key::Num2), '3' => Some(awase::Key::Num3),
            '4' => Some(awase::Key::Num4), '5' => Some(awase::Key::Num5),
            '6' => Some(awase::Key::Num6), '7' => Some(awase::Key::Num7),
            '8' => Some(awase::Key::Num8), '9' => Some(awase::Key::Num9),
            _ => None,
        },
        KeyCode::Space => Some(awase::Key::Space),
        KeyCode::Enter => Some(awase::Key::Return),
        KeyCode::Escape => Some(awase::Key::Escape),
        KeyCode::Tab => Some(awase::Key::Tab),
        KeyCode::Backspace => Some(awase::Key::Backspace),
        KeyCode::Delete => Some(awase::Key::Delete),
        KeyCode::Up => Some(awase::Key::Up),
        KeyCode::Down => Some(awase::Key::Down),
        KeyCode::Left => Some(awase::Key::Left),
        KeyCode::Right => Some(awase::Key::Right),
        _ => None,
    }
}

/// Check if a key event matches an awase hotkey string.
///
/// Parses the hotkey string and compares against the event. This enables
/// config-driven keybinding lookups like:
///
/// ```ignore
/// if matches_hotkey(event, "cmd+n") {
///     // handle next track
/// }
/// ```
#[must_use]
pub fn matches_hotkey(event: &KeyEvent, hotkey_str: &str) -> bool {
    let Some(event_hk) = to_awase_hotkey(event) else {
        return false;
    };
    awase::Hotkey::parse(hotkey_str)
        .map(|parsed| parsed == event_hk)
        .unwrap_or(false)
}

/// Convert a key event into an action based on the current input mode.
#[must_use]
pub fn map_key(event: &KeyEvent, mode: InputMode) -> Action {
    // Only process key presses, not releases.
    if !event.pressed {
        return Action::None;
    }

    match mode {
        InputMode::Normal => map_normal(event),
        InputMode::Library => map_library(event),
        InputMode::Queue => map_queue(event),
        InputMode::Search => map_search(event),
        InputMode::Command => map_command(event),
        InputMode::Torrent => map_torrent(event),
    }
}

/// Normal mode keybindings.
fn map_normal(event: &KeyEvent) -> Action {
    let mods = &event.modifiers;

    match event.key {
        KeyCode::Space => Action::TogglePlay,
        KeyCode::Char('n') if !mods.any() => Action::NextTrack,
        KeyCode::Char('p') if !mods.any() => Action::PrevTrack,
        KeyCode::Char('+') | KeyCode::Char('=') => Action::VolumeUp,
        KeyCode::Char('-') => Action::VolumeDown,
        KeyCode::Char('m') if !mods.any() => Action::ToggleMute,
        KeyCode::Char('s') if !mods.any() => Action::ToggleShuffle,
        KeyCode::Char('r') if !mods.any() => Action::CycleRepeat,
        KeyCode::Char('/') => Action::EnterSearch,
        KeyCode::Char(':') => Action::EnterCommand,
        KeyCode::Char('q') if !mods.any() => Action::Quit,
        KeyCode::Tab if !mods.shift => Action::NextPanel,
        KeyCode::Tab if mods.shift => Action::PrevPanel,
        KeyCode::Char('1') => Action::JumpToPanel(0),
        KeyCode::Char('2') => Action::JumpToPanel(1),
        KeyCode::Char('3') => Action::JumpToPanel(2),
        KeyCode::Char('4') => Action::JumpToPanel(3),
        KeyCode::Char('j') if !mods.any() => Action::MoveDown,
        KeyCode::Char('k') if !mods.any() => Action::MoveUp,
        KeyCode::Char('h') if !mods.any() => Action::MoveLeft,
        KeyCode::Char('l') if !mods.any() => Action::MoveRight,
        KeyCode::Char('.') if mods.shift => Action::SeekForward,
        KeyCode::Char(',') if mods.shift => Action::SeekBackward,
        KeyCode::Enter => Action::Select,
        _ => Action::None,
    }
}

/// Library mode keybindings.
fn map_library(event: &KeyEvent) -> Action {
    let mods = &event.modifiers;

    match event.key {
        KeyCode::Char('j') | KeyCode::Down => Action::MoveDown,
        KeyCode::Char('k') | KeyCode::Up => Action::MoveUp,
        KeyCode::Char('g') if !mods.shift => Action::GoToTop,
        KeyCode::Char('G') if mods.shift => Action::GoToBottom,
        KeyCode::PageDown => Action::PageDown,
        KeyCode::PageUp => Action::PageUp,
        KeyCode::Enter => Action::Select,
        KeyCode::Char('a') if !mods.any() => Action::AddToQueue,
        KeyCode::Char('/') => Action::EnterSearch,
        KeyCode::Escape => Action::ExitMode,
        KeyCode::Space => Action::TogglePlay,
        KeyCode::Char('n') if !mods.any() => Action::NextTrack,
        KeyCode::Char('p') if !mods.any() => Action::PrevTrack,
        KeyCode::Tab => Action::NextPanel,
        KeyCode::Char('q') if !mods.any() => Action::Quit,
        _ => Action::None,
    }
}

/// Queue mode keybindings.
fn map_queue(event: &KeyEvent) -> Action {
    let mods = &event.modifiers;

    match event.key {
        KeyCode::Char('j') | KeyCode::Down => Action::MoveDown,
        KeyCode::Char('k') | KeyCode::Up => Action::MoveUp,
        KeyCode::Char('J') if mods.shift => Action::MoveQueueItemDown,
        KeyCode::Char('K') if mods.shift => Action::MoveQueueItemUp,
        KeyCode::Char('d') if !mods.any() => Action::RemoveFromQueue,
        KeyCode::Char('c') if !mods.any() => Action::ClearQueue,
        KeyCode::Enter => Action::Select,
        KeyCode::Escape => Action::ExitMode,
        KeyCode::Space => Action::TogglePlay,
        KeyCode::Tab => Action::NextPanel,
        KeyCode::Char('q') if !mods.any() => Action::Quit,
        _ => Action::None,
    }
}

/// Torrent mode keybindings.
fn map_torrent(event: &KeyEvent) -> Action {
    match event.key {
        KeyCode::Char('j') | KeyCode::Down => Action::MoveDown,
        KeyCode::Char('k') | KeyCode::Up => Action::MoveUp,
        KeyCode::Escape => Action::ExitMode,
        KeyCode::Space => Action::TogglePlay,
        KeyCode::Tab => Action::NextPanel,
        KeyCode::Char('q') => Action::Quit,
        _ => Action::None,
    }
}

/// Search mode keybindings.
fn map_search(event: &KeyEvent) -> Action {
    match event.key {
        KeyCode::Escape => Action::ExitMode,
        KeyCode::Enter => Action::SubmitSearch,
        KeyCode::Backspace => Action::SearchBackspace,
        KeyCode::Char(c) => Action::SearchChar(c),
        _ => Action::None,
    }
}

/// Command mode keybindings.
fn map_command(event: &KeyEvent) -> Action {
    match event.key {
        KeyCode::Escape => Action::ExitMode,
        KeyCode::Enter => Action::SubmitCommand(String::new()), // command text handled externally
        KeyCode::Backspace => Action::CommandBackspace,
        KeyCode::Char(c) => Action::CommandChar(c),
        _ => Action::None,
    }
}

/// Parse a command string (from `:command` mode) into an action.
#[must_use]
pub fn parse_command(cmd: &str) -> Action {
    let parts: Vec<&str> = cmd.trim().splitn(2, ' ').collect();
    let command = parts.first().copied().unwrap_or("");
    let _arg = parts.get(1).copied().unwrap_or("");

    match command {
        "play" | "p" => Action::TogglePlay,
        "pause" => Action::TogglePlay,
        "stop" => Action::Stop,
        "next" | "n" => Action::NextTrack,
        "prev" => Action::PrevTrack,
        "quit" | "q" => Action::Quit,
        "shuffle" => Action::ToggleShuffle,
        "repeat" => Action::CycleRepeat,
        "mute" => Action::ToggleMute,
        "clear" => Action::ClearQueue,
        _ => Action::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            key: code,
            pressed: true,
            modifiers: Modifiers::default(),
            text: None,
        }
    }

    fn key_with_shift(code: KeyCode) -> KeyEvent {
        KeyEvent {
            key: code,
            pressed: true,
            modifiers: Modifiers {
                shift: true,
                ..Default::default()
            },
            text: None,
        }
    }

    fn key_released(code: KeyCode) -> KeyEvent {
        KeyEvent {
            key: code,
            pressed: false,
            modifiers: Modifiers::default(),
            text: None,
        }
    }

    #[test]
    fn normal_mode_playback() {
        assert_eq!(map_key(&key(KeyCode::Space), InputMode::Normal), Action::TogglePlay);
        assert_eq!(map_key(&key(KeyCode::Char('n')), InputMode::Normal), Action::NextTrack);
        assert_eq!(map_key(&key(KeyCode::Char('p')), InputMode::Normal), Action::PrevTrack);
    }

    #[test]
    fn normal_mode_volume() {
        assert_eq!(map_key(&key(KeyCode::Char('+')), InputMode::Normal), Action::VolumeUp);
        assert_eq!(map_key(&key(KeyCode::Char('-')), InputMode::Normal), Action::VolumeDown);
        assert_eq!(map_key(&key(KeyCode::Char('m')), InputMode::Normal), Action::ToggleMute);
    }

    #[test]
    fn normal_mode_shuffle_repeat() {
        assert_eq!(map_key(&key(KeyCode::Char('s')), InputMode::Normal), Action::ToggleShuffle);
        assert_eq!(map_key(&key(KeyCode::Char('r')), InputMode::Normal), Action::CycleRepeat);
    }

    #[test]
    fn normal_mode_navigation() {
        assert_eq!(map_key(&key(KeyCode::Tab), InputMode::Normal), Action::NextPanel);
        assert_eq!(map_key(&key(KeyCode::Char('/')), InputMode::Normal), Action::EnterSearch);
        assert_eq!(map_key(&key(KeyCode::Char(':')), InputMode::Normal), Action::EnterCommand);
        assert_eq!(map_key(&key(KeyCode::Char('q')), InputMode::Normal), Action::Quit);
    }

    #[test]
    fn normal_mode_panel_jump() {
        assert_eq!(map_key(&key(KeyCode::Char('1')), InputMode::Normal), Action::JumpToPanel(0));
        assert_eq!(map_key(&key(KeyCode::Char('2')), InputMode::Normal), Action::JumpToPanel(1));
        assert_eq!(map_key(&key(KeyCode::Char('3')), InputMode::Normal), Action::JumpToPanel(2));
        assert_eq!(map_key(&key(KeyCode::Char('4')), InputMode::Normal), Action::JumpToPanel(3));
    }

    #[test]
    fn library_mode() {
        assert_eq!(map_key(&key(KeyCode::Char('j')), InputMode::Library), Action::MoveDown);
        assert_eq!(map_key(&key(KeyCode::Char('k')), InputMode::Library), Action::MoveUp);
        assert_eq!(map_key(&key(KeyCode::Enter), InputMode::Library), Action::Select);
        assert_eq!(map_key(&key(KeyCode::Char('a')), InputMode::Library), Action::AddToQueue);
        assert_eq!(map_key(&key(KeyCode::Escape), InputMode::Library), Action::ExitMode);
    }

    #[test]
    fn queue_mode() {
        assert_eq!(map_key(&key(KeyCode::Char('d')), InputMode::Queue), Action::RemoveFromQueue);
        assert_eq!(map_key(&key(KeyCode::Char('c')), InputMode::Queue), Action::ClearQueue);
        assert_eq!(
            map_key(&key_with_shift(KeyCode::Char('J')), InputMode::Queue),
            Action::MoveQueueItemDown
        );
        assert_eq!(
            map_key(&key_with_shift(KeyCode::Char('K')), InputMode::Queue),
            Action::MoveQueueItemUp
        );
    }

    #[test]
    fn search_mode() {
        assert_eq!(
            map_key(&key(KeyCode::Char('a')), InputMode::Search),
            Action::SearchChar('a')
        );
        assert_eq!(map_key(&key(KeyCode::Backspace), InputMode::Search), Action::SearchBackspace);
        assert_eq!(map_key(&key(KeyCode::Enter), InputMode::Search), Action::SubmitSearch);
        assert_eq!(map_key(&key(KeyCode::Escape), InputMode::Search), Action::ExitMode);
    }

    #[test]
    fn command_mode() {
        assert_eq!(
            map_key(&key(KeyCode::Char('x')), InputMode::Command),
            Action::CommandChar('x')
        );
        assert_eq!(
            map_key(&key(KeyCode::Backspace), InputMode::Command),
            Action::CommandBackspace
        );
        assert_eq!(map_key(&key(KeyCode::Escape), InputMode::Command), Action::ExitMode);
    }

    #[test]
    fn key_release_ignored() {
        assert_eq!(
            map_key(&key_released(KeyCode::Space), InputMode::Normal),
            Action::None
        );
    }

    #[test]
    fn parse_command_known() {
        assert_eq!(parse_command("play"), Action::TogglePlay);
        assert_eq!(parse_command("next"), Action::NextTrack);
        assert_eq!(parse_command("prev"), Action::PrevTrack);
        assert_eq!(parse_command("quit"), Action::Quit);
        assert_eq!(parse_command("shuffle"), Action::ToggleShuffle);
        assert_eq!(parse_command("clear"), Action::ClearQueue);
        assert_eq!(parse_command("stop"), Action::Stop);
    }

    #[test]
    fn parse_command_unknown() {
        assert_eq!(parse_command("notacommand"), Action::None);
        assert_eq!(parse_command(""), Action::None);
    }

    #[test]
    fn parse_command_aliases() {
        assert_eq!(parse_command("p"), Action::TogglePlay);
        assert_eq!(parse_command("n"), Action::NextTrack);
        assert_eq!(parse_command("q"), Action::Quit);
    }

    // -- awase integration tests --

    #[test]
    fn to_awase_hotkey_converts_char() {
        let event = key(KeyCode::Char('j'));
        let hk = to_awase_hotkey(&event).unwrap();
        assert_eq!(hk.key, awase::Key::J);
        assert!(hk.modifiers.is_empty());
    }

    #[test]
    fn to_awase_hotkey_converts_space() {
        let event = key(KeyCode::Space);
        let hk = to_awase_hotkey(&event).unwrap();
        assert_eq!(hk.key, awase::Key::Space);
    }

    #[test]
    fn to_awase_hotkey_with_shift() {
        let event = key_with_shift(KeyCode::Char('J'));
        let hk = to_awase_hotkey(&event).unwrap();
        assert_eq!(hk.key, awase::Key::J);
        assert!(hk.modifiers.contains(awase::Modifiers::SHIFT));
    }

    #[test]
    fn to_awase_hotkey_unconvertible() {
        let event = key(KeyCode::PageUp);
        assert!(to_awase_hotkey(&event).is_none());
    }

    #[test]
    fn matches_hotkey_basic() {
        let event = key(KeyCode::Space);
        assert!(matches_hotkey(&event, "space"));
        assert!(!matches_hotkey(&event, "return"));
    }

    #[test]
    fn matches_hotkey_with_modifier() {
        let event = KeyEvent {
            key: KeyCode::Char('n'),
            pressed: true,
            modifiers: Modifiers {
                ctrl: true,
                ..Default::default()
            },
            text: None,
        };
        assert!(matches_hotkey(&event, "ctrl+n"));
        assert!(!matches_hotkey(&event, "n"));
    }

    #[test]
    fn matches_hotkey_invalid_string() {
        let event = key(KeyCode::Space);
        assert!(!matches_hotkey(&event, "invalidkey!!!"));
    }

    #[test]
    fn awase_hotkey_parse_roundtrip() {
        let hk = awase::Hotkey::parse("cmd+space").unwrap();
        assert_eq!(hk.modifiers, awase::Modifiers::CMD);
        assert_eq!(hk.key, awase::Key::Space);
    }
}
