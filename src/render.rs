//! GPU rendering module -- wgpu pipeline for player UI.
//!
//! Uses garasu for GPU context, text rendering, and shader pipeline.
//! Renders: album art, waveform visualizer, track list, controls.
//!
//! # Architecture
//!
//! The rendering pipeline is structured as follows:
//!
//! ```text
//! AudioEngine state
//!       |
//!       v
//! UI state (current track, queue, volume, playback position)
//!       |
//!       v
//! garasu Scene graph
//!   - Album art texture (decoded via image crate)
//!   - Track list (mojiban rich text)
//!   - Playback controls (egaku widgets)
//!   - Waveform visualizer (custom shader)
//!   - Volume slider (egaku widget)
//!       |
//!       v
//! wgpu render pass -> swapchain present
//! ```
//!
//! # Planned Components
//!
//! - **Album art panel**: Displays cover art for the current track, decoded from
//!   embedded metadata or companion image files (cover.jpg, folder.png).
//!
//! - **Track list**: Scrollable list of queued tracks rendered with mojiban. The
//!   current track is highlighted with the accent colour from configuration.
//!
//! - **Playback controls**: Play/pause, next, previous, shuffle, repeat buttons
//!   built with egaku widget primitives.
//!
//! - **Waveform visualizer**: Real-time waveform or spectrum display rendered
//!   via a custom garasu shader. Receives PCM samples from the audio engine.
//!
//! - **Progress bar**: Seek bar showing current position within the track.
//!
//! - **Volume control**: Vertical or horizontal slider for volume adjustment.
//!
//! - **Library browser**: Tab for browsing the music library by artist, album,
//!   or search query. Uses mojiban for styled text and egaku for list/grid layout.
//!
//! - **Torrent panel**: Shows active downloads, progress bars, peer counts.
//!   Integrates with the torrent module for real-time status updates.
//!
//! # Dependencies
//!
//! - `garasu`: GPU context management, texture loading, shader pipeline
//! - `egaku`: Widget toolkit for controls, sliders, buttons, lists
//! - `mojiban`: Rich text rendering for track metadata and library browser
//! - `wgpu`: Low-level GPU API (managed by garasu)
//! - `winit`: Window creation and event loop (managed by garasu)
