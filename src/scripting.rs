//! Rhai scripting integration for Hibiki.
//!
//! Loads user scripts from `~/.config/hibiki/scripts/*.rhai` and exposes
//! app-specific functions for playback control and library queries.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use soushi::ScriptEngine;

/// Script hook events that can trigger user scripts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptEvent {
    /// A track started playing.
    TrackStarted { title: String, artist: String },
    /// A track finished playing.
    TrackFinished { title: String },
    /// Playback was paused.
    Paused,
    /// Playback was resumed.
    Resumed,
    /// The queue was modified.
    QueueChanged { count: usize },
}

/// Manages the Rhai scripting engine with hibiki-specific functions.
pub struct HibikiScripting {
    engine: ScriptEngine,
    /// Compiled event hook scripts (ASTs keyed by event name).
    hooks: std::collections::HashMap<String, soushi::rhai::AST>,
}

impl HibikiScripting {
    /// Create a new scripting engine with hibiki playback functions registered.
    ///
    /// Registers: `hibiki.play()`, `hibiki.pause()`, `hibiki.next()`,
    /// `hibiki.search(query)`, `hibiki.queue_add(path)`.
    ///
    /// The `action_tx` channel is used to send actions back to the main event loop.
    #[must_use]
    pub fn new(action_tx: Arc<Mutex<Vec<ScriptAction>>>) -> Self {
        let mut engine = ScriptEngine::new();
        engine.register_builtin_log();
        engine.register_builtin_env();
        engine.register_builtin_string();

        // hibiki.play()
        let tx = action_tx.clone();
        engine.register_fn("hibiki_play", move || {
            if let Ok(mut actions) = tx.lock() {
                actions.push(ScriptAction::Play);
            }
        });

        // hibiki.pause()
        let tx = action_tx.clone();
        engine.register_fn("hibiki_pause", move || {
            if let Ok(mut actions) = tx.lock() {
                actions.push(ScriptAction::Pause);
            }
        });

        // hibiki.next()
        let tx = action_tx.clone();
        engine.register_fn("hibiki_next", move || {
            if let Ok(mut actions) = tx.lock() {
                actions.push(ScriptAction::Next);
            }
        });

        // hibiki.search(query)
        let tx = action_tx.clone();
        engine.register_fn("hibiki_search", move |query: &str| {
            if let Ok(mut actions) = tx.lock() {
                actions.push(ScriptAction::Search(query.to_string()));
            }
        });

        // hibiki.queue_add(path)
        let tx = action_tx;
        engine.register_fn("hibiki_queue_add", move |path: &str| {
            if let Ok(mut actions) = tx.lock() {
                actions.push(ScriptAction::QueueAdd(PathBuf::from(path)));
            }
        });

        Self {
            engine,
            hooks: std::collections::HashMap::new(),
        }
    }

    /// Load all scripts from the scripts directory.
    ///
    /// Looks in `~/.config/hibiki/scripts/` by default.
    pub fn load_scripts(&mut self) -> Result<Vec<String>, soushi::SoushiError> {
        let scripts_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("hibiki")
            .join("scripts");

        if !scripts_dir.is_dir() {
            tracing::debug!(path = %scripts_dir.display(), "scripts directory not found, skipping");
            return Ok(Vec::new());
        }

        self.engine.load_scripts_dir(&scripts_dir)
    }

    /// Register an event hook script.
    ///
    /// The script will be evaluated when the named event fires.
    pub fn register_hook(&mut self, event_name: &str, script: &str) -> Result<(), soushi::SoushiError> {
        let ast = self.engine.compile(script)?;
        self.hooks.insert(event_name.to_string(), ast);
        Ok(())
    }

    /// Fire an event, running any registered hook scripts.
    pub fn fire_event(&self, event: &ScriptEvent) {
        let event_name = match event {
            ScriptEvent::TrackStarted { .. } => "track_started",
            ScriptEvent::TrackFinished { .. } => "track_finished",
            ScriptEvent::Paused => "paused",
            ScriptEvent::Resumed => "resumed",
            ScriptEvent::QueueChanged { .. } => "queue_changed",
        };

        if let Some(ast) = self.hooks.get(event_name) {
            if let Err(e) = self.engine.eval_ast(ast) {
                tracing::error!(event = event_name, error = %e, "script hook failed");
            }
        }
    }

    /// Evaluate an ad-hoc script string.
    pub fn eval(&self, script: &str) -> Result<soushi::rhai::Dynamic, soushi::SoushiError> {
        self.engine.eval(script)
    }
}

/// Actions that scripts can request from the application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptAction {
    /// Start or resume playback.
    Play,
    /// Pause playback.
    Pause,
    /// Skip to next track.
    Next,
    /// Search the library.
    Search(String),
    /// Add a file to the queue.
    QueueAdd(PathBuf),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_engine() -> (HibikiScripting, Arc<Mutex<Vec<ScriptAction>>>) {
        let actions = Arc::new(Mutex::new(Vec::new()));
        let engine = HibikiScripting::new(actions.clone());
        (engine, actions)
    }

    #[test]
    fn play_function_queues_action() {
        let (engine, actions) = make_engine();
        engine.eval("hibiki_play()").unwrap();
        let actions = actions.lock().unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0], ScriptAction::Play);
    }

    #[test]
    fn pause_function_queues_action() {
        let (engine, actions) = make_engine();
        engine.eval("hibiki_pause()").unwrap();
        let actions = actions.lock().unwrap();
        assert_eq!(actions[0], ScriptAction::Pause);
    }

    #[test]
    fn next_function_queues_action() {
        let (engine, actions) = make_engine();
        engine.eval("hibiki_next()").unwrap();
        let actions = actions.lock().unwrap();
        assert_eq!(actions[0], ScriptAction::Next);
    }

    #[test]
    fn search_function_queues_action() {
        let (engine, actions) = make_engine();
        engine.eval(r#"hibiki_search("beethoven")"#).unwrap();
        let actions = actions.lock().unwrap();
        assert_eq!(actions[0], ScriptAction::Search("beethoven".to_string()));
    }

    #[test]
    fn queue_add_function_queues_action() {
        let (engine, actions) = make_engine();
        engine.eval(r#"hibiki_queue_add("/music/song.flac")"#).unwrap();
        let actions = actions.lock().unwrap();
        assert_eq!(
            actions[0],
            ScriptAction::QueueAdd(PathBuf::from("/music/song.flac"))
        );
    }

    #[test]
    fn fire_event_with_no_hook_is_noop() {
        let (engine, _actions) = make_engine();
        engine.fire_event(&ScriptEvent::Paused);
        // No panic, no error.
    }

    #[test]
    fn register_and_fire_hook() {
        let (mut engine, actions) = make_engine();
        engine
            .register_hook("paused", "hibiki_play()")
            .unwrap();
        engine.fire_event(&ScriptEvent::Paused);
        let actions = actions.lock().unwrap();
        assert_eq!(actions[0], ScriptAction::Play);
    }

    #[test]
    fn load_scripts_missing_dir_returns_empty() {
        let (mut engine, _actions) = make_engine();
        // With a default config dir, the scripts subdir likely doesn't exist.
        // This should not error, just return empty.
        let result = engine.load_scripts();
        assert!(result.is_ok());
    }

    #[test]
    fn eval_arbitrary_script() {
        let (engine, _actions) = make_engine();
        let result = engine.eval("40 + 2").unwrap();
        assert_eq!(result.as_int().unwrap(), 42);
    }
}
