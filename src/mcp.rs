//! MCP server for hibiki music player automation.
//!
//! Tools:
//!   `status`         — current playback status (state, track, position, volume)
//!   `version`        — server version info
//!   `config_get`     — get a config value by key
//!   `config_set`     — set a config value
//!   `play`           — start or resume playback
//!   `pause`          — pause playback
//!   `next`           — skip to next track
//!   `prev`           — skip to previous track
//!   `queue_list`     — list tracks in the queue
//!   `search_library` — search the music library
//!   `now_playing`    — get info about the currently playing track
//!   `set_volume`     — set playback volume (0.0–1.0)

use kaname::rmcp::{
    ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
    transport::stdio,
};
use serde::Deserialize;
use serde_json::json;

// ── Tool input types ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ConfigGetInput {
    #[schemars(description = "Config key to retrieve (e.g. 'music_dir', 'audio.sample_rate', 'torrent.dht_enabled').")]
    key: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ConfigSetInput {
    #[schemars(description = "Config key to set.")]
    key: String,
    #[schemars(description = "New value as a JSON string.")]
    value: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SearchLibraryInput {
    #[schemars(description = "Search query (matches artist, album, or title).")]
    query: String,
    #[schemars(description = "Maximum number of results to return.")]
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SetVolumeInput {
    #[schemars(description = "Volume level from 0.0 (mute) to 1.0 (maximum).")]
    volume: f32,
}

// ── MCP Server ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct HibikiMcp {
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl HibikiMcp {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    // ── Standard tools ──────────────────────────────────────────────────────

    #[tool(description = "Get current playback status: state (playing/paused/stopped), track info, position, volume, queue length.")]
    async fn status(&self) -> String {
        // TODO: connect to running daemon via tsunagu IPC
        serde_json::to_string(&json!({
            "state": "stopped",
            "track": null,
            "position_secs": 0,
            "volume": 1.0,
            "muted": false,
            "shuffle": false,
            "repeat": "off",
            "queue_length": 0
        }))
        .unwrap_or_default()
    }

    #[tool(description = "Get hibiki version information.")]
    async fn version(&self) -> String {
        serde_json::to_string(&json!({
            "name": "hibiki",
            "crate": "hibikine",
            "version": env!("CARGO_PKG_VERSION"),
        }))
        .unwrap_or_default()
    }

    #[tool(description = "Get a configuration value by key (e.g. 'music_dir', 'audio.sample_rate').")]
    async fn config_get(&self, Parameters(input): Parameters<ConfigGetInput>) -> String {
        // TODO: read from shikumi ConfigStore
        serde_json::to_string(&json!({
            "key": input.key,
            "value": null,
            "error": "config store not connected (daemon not running)"
        }))
        .unwrap_or_default()
    }

    #[tool(description = "Set a configuration value. Changes are applied immediately via hot-reload.")]
    async fn config_set(&self, Parameters(input): Parameters<ConfigSetInput>) -> String {
        // TODO: write to shikumi ConfigStore
        serde_json::to_string(&json!({
            "key": input.key,
            "value": input.value,
            "applied": false,
            "error": "config store not connected (daemon not running)"
        }))
        .unwrap_or_default()
    }

    // ── Playback tools ──────────────────────────────────────────────────────

    #[tool(description = "Start or resume playback. If stopped, plays the first track in the queue.")]
    async fn play(&self) -> String {
        // TODO: send play command via tsunagu IPC
        serde_json::to_string(&json!({
            "ok": false,
            "error": "daemon not running"
        }))
        .unwrap_or_default()
    }

    #[tool(description = "Pause the current playback.")]
    async fn pause(&self) -> String {
        // TODO: send pause command via tsunagu IPC
        serde_json::to_string(&json!({
            "ok": false,
            "error": "daemon not running"
        }))
        .unwrap_or_default()
    }

    #[tool(description = "Skip to the next track in the queue.")]
    async fn next(&self) -> String {
        // TODO: send next command via tsunagu IPC
        serde_json::to_string(&json!({
            "ok": false,
            "error": "daemon not running"
        }))
        .unwrap_or_default()
    }

    #[tool(description = "Skip to the previous track in the queue.")]
    async fn prev(&self) -> String {
        // TODO: send prev command via tsunagu IPC
        serde_json::to_string(&json!({
            "ok": false,
            "error": "daemon not running"
        }))
        .unwrap_or_default()
    }

    #[tool(description = "List all tracks currently in the playback queue.")]
    async fn queue_list(&self) -> String {
        // TODO: query queue via tsunagu IPC
        serde_json::to_string(&json!({
            "tracks": [],
            "current_index": null,
            "total": 0
        }))
        .unwrap_or_default()
    }

    #[tool(description = "Search the music library by artist, album, or track title.")]
    async fn search_library(&self, Parameters(input): Parameters<SearchLibraryInput>) -> String {
        let limit = input.limit.unwrap_or(20);
        // TODO: query library via tsunagu IPC
        serde_json::to_string(&json!({
            "query": input.query,
            "limit": limit,
            "results": [],
            "total": 0
        }))
        .unwrap_or_default()
    }

    #[tool(description = "Get detailed info about the currently playing track: title, artist, album, duration, format, bitrate.")]
    async fn now_playing(&self) -> String {
        // TODO: query now-playing via tsunagu IPC
        serde_json::to_string(&json!({
            "playing": false,
            "track": null
        }))
        .unwrap_or_default()
    }

    #[tool(description = "Set the playback volume. Range: 0.0 (mute) to 1.0 (maximum).")]
    async fn set_volume(&self, Parameters(input): Parameters<SetVolumeInput>) -> String {
        let volume = input.volume.clamp(0.0, 1.0);
        // TODO: send volume command via tsunagu IPC
        serde_json::to_string(&json!({
            "ok": false,
            "volume": volume,
            "error": "daemon not running"
        }))
        .unwrap_or_default()
    }
}

#[tool_handler]
impl ServerHandler for HibikiMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Hibiki music player — playback control, queue management, and library search."
                    .into(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let server = HibikiMcp::new().serve(stdio()).await?;
    server.waiting().await?;
    Ok(())
}
