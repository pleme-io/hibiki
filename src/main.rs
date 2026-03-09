//! Hibiki (響) — GPU-rendered music player with built-in BitTorrent.
//!
//! A high-fidelity music player that combines:
//! - GPU-accelerated UI via garasu (wgpu/winit)
//! - Lossless audio playback via rodio + symphonia (FLAC, ALAC, WAV, etc.)
//! - Built-in BitTorrent client for discovering and downloading music
//! - Library management with metadata extraction (lofty)
//! - Hot-reloadable configuration via shikumi
//! - Vim-style keyboard navigation

mod audio;
mod config;
mod input;
mod library;
mod mcp;
mod render;
mod torrent;

use clap::{Parser, Subcommand};
use std::sync::{Arc, Mutex};
use tracing_subscriber::EnvFilter;

use crate::audio::{AudioEngine, Track};
use crate::input::{Action, InputMode};
use crate::library::Library;
use crate::render::{HibikiRenderer, Panel};
use crate::torrent::TorrentClient;

#[derive(Parser)]
#[command(name = "hibiki", version, about = "GPU-rendered music player + BitTorrent")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Configuration file override
    #[arg(long, env = "HIBIKI_CONFIG")]
    config: Option<std::path::PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    /// Launch the GUI player
    Play {
        /// File or directory to play
        path: Option<std::path::PathBuf>,
    },
    /// Start the background daemon (library indexer + torrent client)
    Daemon,
    /// Add a torrent (magnet link or .torrent file)
    Add {
        /// Magnet URI or path to .torrent file
        source: String,
    },
    /// List active torrents
    Torrents,
    /// Scan and index music library
    Scan {
        /// Directory to scan (default: configured music_dir)
        path: Option<std::path::PathBuf>,
    },
    /// Start MCP server (stdio transport)
    Mcp,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    let cfg = config::load(&cli.config)?;

    match cli.command {
        None | Some(Commands::Play { .. }) => {
            let play_path = match &cli.command {
                Some(Commands::Play { path }) => path.clone(),
                _ => None,
            };
            run_player(cfg, play_path)?;
        }
        Some(Commands::Daemon) => {
            tracing::info!("starting hibiki daemon");
            run_daemon(cfg)?;
        }
        Some(Commands::Add { source }) => {
            tracing::info!("adding torrent: {source}");
            run_add_torrent(cfg, &source)?;
        }
        Some(Commands::Torrents) => {
            run_list_torrents(cfg)?;
        }
        Some(Commands::Scan { path }) => {
            let scan_dir = path.unwrap_or_else(|| cfg.music_dir.clone());
            tracing::info!("scanning library: {}", scan_dir.display());
            run_scan(cfg, &scan_dir)?;
        }
        Some(Commands::Mcp) => {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?;
            rt.block_on(async {
                mcp::run().await.map_err(|e| anyhow::anyhow!("MCP server error: {e}"))
            })?;
        }
    }

    Ok(())
}

/// Message sent from background threads to the main event loop.
enum BgMessage {
    /// Tracks scanned and ready to be added to the library.
    TracksScanned(Vec<Track>),
    /// Tracks to enqueue and immediately play.
    EnqueueAndPlay(Vec<Track>),
}

/// Launch the GUI player with madori.
fn run_player(
    cfg: config::HibikiConfig,
    play_path: Option<std::path::PathBuf>,
) -> anyhow::Result<()> {
    tracing::info!("launching hibiki player");

    // Channel for background tasks to communicate with the main thread.
    let (bg_tx, bg_rx) = std::sync::mpsc::channel::<BgMessage>();

    // Library is Send-safe, shared between main and scanner threads.
    let library = Arc::new(Mutex::new(Library::new()));

    // Scan library on startup if configured.
    if cfg.library.scan_on_startup {
        let lib = library.clone();
        let music_dir = cfg.music_dir.clone();
        let tx = bg_tx.clone();

        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                let mut lib = lib.lock().unwrap();
                match lib.scan(&music_dir).await {
                    Ok(count) => {
                        tracing::info!(count, "library scan complete");
                        let tracks = lib.tracks().to_vec();
                        let _ = tx.send(BgMessage::TracksScanned(tracks));
                    }
                    Err(e) => {
                        tracing::error!("library scan failed: {e}");
                    }
                }
            });
        });
    }

    // If a path was provided, scan and queue its contents in a background thread.
    if let Some(path) = play_path {
        let lib = library.clone();
        let tx = bg_tx.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                let mut lib_guard = lib.lock().unwrap();
                if path.is_dir() {
                    if let Ok(count) = lib_guard.scan(&path).await {
                        tracing::info!(count, "scanned play path");
                        let tracks = lib_guard.tracks().to_vec();
                        let _ = tx.send(BgMessage::EnqueueAndPlay(tracks));
                    }
                } else if audio::is_audio_file(&path) {
                    let track = library::extract_metadata(&path);
                    let _ = tx.send(BgMessage::EnqueueAndPlay(vec![track]));
                }
            });
        });
    }

    // Drop the sender so the channel closes when all background threads finish.
    drop(bg_tx);

    // Create the renderer.
    let renderer = HibikiRenderer::new(
        &cfg.appearance.background,
        &cfg.appearance.foreground,
        &cfg.appearance.accent,
    );

    // Audio engine lives only in the event handler closure (main thread).
    let mut engine = AudioEngine::new(&cfg.audio);
    let library_for_events = library.clone();

    // Build and run the madori app.
    madori::App::builder(renderer)
        .title("Hibiki")
        .size(1280, 720)
        .on_event(move |event, renderer| {
            use madori::event::AppEvent;

            // Process any pending background messages.
            while let Ok(msg) = bg_rx.try_recv() {
                match msg {
                    BgMessage::TracksScanned(_tracks) => {
                        // Library already updated via shared Arc<Mutex<Library>>.
                        tracing::debug!("background scan results received");
                    }
                    BgMessage::EnqueueAndPlay(tracks) => {
                        engine.enqueue_many(tracks);
                        let _ = engine.play();
                    }
                }
            }

            match event {
                AppEvent::Key(key_event) => {
                    let action = input::map_key(key_event, renderer.ui.mode);
                    handle_action(
                        action,
                        renderer,
                        &mut engine,
                        &library_for_events,
                    )
                }
                AppEvent::RedrawRequested => {
                    // Auto-advance to next track when current finishes.
                    engine.tick();
                    renderer.update_from_engine(&engine);

                    // Update library list.
                    if let Ok(lib) = library.lock() {
                        if renderer.ui.search_query.is_empty() {
                            renderer.ui.update_library_list(lib.tracks());
                        } else {
                            let results = lib.search(&renderer.ui.search_query);
                            let tracks: Vec<audio::Track> =
                                results.into_iter().cloned().collect();
                            renderer.ui.update_library_list(&tracks);
                        }
                    }
                    madori::EventResponse::ignored()
                }
                AppEvent::Resized { width, height } => {
                    renderer.ui.width = *width;
                    renderer.ui.height = *height;
                    madori::EventResponse::ignored()
                }
                AppEvent::CloseRequested => {
                    engine.stop();
                    madori::EventResponse::ignored()
                }
                _ => madori::EventResponse::ignored(),
            }
        })
        .run()
        .map_err(|e| anyhow::anyhow!("player error: {e}"))?;

    Ok(())
}

/// Handle a resolved input action.
fn handle_action(
    action: Action,
    renderer: &mut HibikiRenderer,
    engine: &mut AudioEngine,
    library: &Arc<Mutex<Library>>,
) -> madori::EventResponse {
    match action {
        // -- Playback --
        Action::TogglePlay => {
            let _ = engine.toggle();
            madori::EventResponse::consumed()
        }
        Action::Stop => {
            engine.stop();
            madori::EventResponse::consumed()
        }
        Action::NextTrack => {
            let _ = engine.next();
            madori::EventResponse::consumed()
        }
        Action::PrevTrack => {
            let _ = engine.previous();
            madori::EventResponse::consumed()
        }
        Action::VolumeUp => {
            engine.adjust_volume(0.05);
            madori::EventResponse::consumed()
        }
        Action::VolumeDown => {
            engine.adjust_volume(-0.05);
            madori::EventResponse::consumed()
        }
        Action::ToggleMute => {
            engine.toggle_mute();
            madori::EventResponse::consumed()
        }
        Action::ToggleShuffle => {
            engine.toggle_shuffle();
            madori::EventResponse::consumed()
        }
        Action::CycleRepeat => {
            engine.cycle_repeat();
            madori::EventResponse::consumed()
        }
        Action::SeekForward => {
            let pos = engine.position() + std::time::Duration::from_secs(10);
            let _ = engine.seek(pos);
            madori::EventResponse::consumed()
        }
        Action::SeekBackward => {
            let pos = engine.position().saturating_sub(std::time::Duration::from_secs(10));
            let _ = engine.seek(pos);
            madori::EventResponse::consumed()
        }

        // -- Navigation --
        Action::MoveDown => {
            match renderer.ui.active_panel {
                Panel::Library => renderer.ui.library_list.select_next(),
                Panel::Queue => renderer.ui.queue_list.select_next(),
                _ => {}
            }
            madori::EventResponse::consumed()
        }
        Action::MoveUp => {
            match renderer.ui.active_panel {
                Panel::Library => renderer.ui.library_list.select_prev(),
                Panel::Queue => renderer.ui.queue_list.select_prev(),
                _ => {}
            }
            madori::EventResponse::consumed()
        }
        Action::GoToTop => {
            match renderer.ui.active_panel {
                Panel::Library => renderer.ui.library_list.select_first(),
                Panel::Queue => renderer.ui.queue_list.select_first(),
                _ => {}
            }
            madori::EventResponse::consumed()
        }
        Action::GoToBottom => {
            match renderer.ui.active_panel {
                Panel::Library => renderer.ui.library_list.select_last(),
                Panel::Queue => renderer.ui.queue_list.select_last(),
                _ => {}
            }
            madori::EventResponse::consumed()
        }
        Action::NextPanel => {
            renderer.ui.next_panel();
            madori::EventResponse::consumed()
        }
        Action::PrevPanel => {
            renderer.ui.prev_panel();
            madori::EventResponse::consumed()
        }
        Action::JumpToPanel(idx) => {
            renderer.ui.switch_panel(Panel::from_index(idx));
            madori::EventResponse::consumed()
        }

        // -- Selection --
        Action::Select => {
            match renderer.ui.active_panel {
                Panel::Library => {
                    let selected = renderer.ui.library_list.selected_index();
                    if let Ok(lib) = library.lock() {
                        if let Some(track) = lib.get_track(selected) {
                            engine.enqueue(track.clone());
                            let idx = engine.queue().len() - 1;
                            let _ = engine.play_index(idx);
                        }
                    }
                }
                Panel::Queue => {
                    let selected = renderer.ui.queue_list.selected_index();
                    let _ = engine.play_index(selected);
                }
                _ => {}
            }
            madori::EventResponse::consumed()
        }
        Action::AddToQueue => {
            let selected = renderer.ui.library_list.selected_index();
            if let Ok(lib) = library.lock() {
                if let Some(track) = lib.get_track(selected) {
                    engine.enqueue(track.clone());
                }
            }
            madori::EventResponse::consumed()
        }
        Action::RemoveFromQueue => {
            let selected = renderer.ui.queue_list.selected_index();
            engine.remove_from_queue(selected);
            madori::EventResponse::consumed()
        }
        Action::MoveQueueItemUp => {
            let selected = renderer.ui.queue_list.selected_index();
            if selected > 0 {
                engine.move_in_queue(selected, selected - 1);
                renderer.ui.queue_list.select_prev();
            }
            madori::EventResponse::consumed()
        }
        Action::MoveQueueItemDown => {
            let selected = renderer.ui.queue_list.selected_index();
            if selected + 1 < engine.queue().len() {
                engine.move_in_queue(selected, selected + 1);
            }
            renderer.ui.queue_list.select_next();
            madori::EventResponse::consumed()
        }
        Action::ClearQueue => {
            engine.clear_queue();
            madori::EventResponse::consumed()
        }

        // -- Mode switching --
        Action::EnterSearch => {
            renderer.ui.mode = InputMode::Search;
            renderer.ui.search_query.clear();
            madori::EventResponse::consumed()
        }
        Action::EnterCommand => {
            renderer.ui.mode = InputMode::Command;
            renderer.ui.command_input = egaku::TextInput::new();
            madori::EventResponse::consumed()
        }
        Action::ExitMode => {
            renderer.ui.mode = match renderer.ui.active_panel {
                Panel::Library => InputMode::Library,
                Panel::Queue => InputMode::Queue,
                Panel::Torrent => InputMode::Torrent,
                Panel::Player => InputMode::Normal,
            };
            madori::EventResponse::consumed()
        }

        // -- Search input --
        Action::SearchChar(c) => {
            renderer.ui.search_input.insert_char(c);
            renderer.ui.search_query = renderer.ui.search_input.text().to_string();
            madori::EventResponse::consumed()
        }
        Action::SearchBackspace => {
            renderer.ui.search_input.delete_back();
            renderer.ui.search_query = renderer.ui.search_input.text().to_string();
            madori::EventResponse::consumed()
        }
        Action::SearchClear => {
            renderer.ui.search_query.clear();
            renderer.ui.search_input = egaku::TextInput::new();
            madori::EventResponse::consumed()
        }
        Action::SubmitSearch => {
            renderer.ui.mode = InputMode::Library;
            renderer.ui.switch_panel(Panel::Library);
            madori::EventResponse::consumed()
        }

        // -- Command input --
        Action::CommandChar(c) => {
            renderer.ui.command_input.insert_char(c);
            madori::EventResponse::consumed()
        }
        Action::CommandBackspace => {
            renderer.ui.command_input.delete_back();
            madori::EventResponse::consumed()
        }
        Action::SubmitCommand(_) => {
            let cmd = renderer.ui.command_input.text().to_string();
            renderer.ui.mode = InputMode::Normal;
            let parsed = input::parse_command(&cmd);
            if parsed != Action::None {
                return handle_action(parsed, renderer, engine, library);
            }
            madori::EventResponse::consumed()
        }

        // -- Quit --
        Action::Quit => {
            engine.stop();
            madori::EventResponse {
                consumed: true,
                exit: true,
                set_title: None,
            }
        }

        Action::None | Action::MoveLeft | Action::MoveRight | Action::PageUp | Action::PageDown => {
            madori::EventResponse::ignored()
        }
    }
}

/// Run the scan command (CLI mode).
fn run_scan(_cfg: config::HibikiConfig, scan_dir: &std::path::Path) -> anyhow::Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    rt.block_on(async {
        let mut lib = Library::new();
        let count = lib.scan(scan_dir).await?;
        println!("Scanned {count} tracks from {}", scan_dir.display());

        let stats = lib.stats();
        println!(
            "Library: {} tracks, {} artists, {} albums",
            stats.total_tracks, stats.total_artists, stats.total_albums
        );
        println!(
            "Total duration: {}",
            audio::format_duration(stats.total_duration)
        );

        Ok(())
    })
}

/// Run the add torrent command (CLI mode).
fn run_add_torrent(cfg: config::HibikiConfig, source: &str) -> anyhow::Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    rt.block_on(async {
        let mut client = TorrentClient::new(&cfg.torrent);

        let id = if source.starts_with("magnet:") {
            client.add_magnet(source).await?
        } else {
            client
                .add_torrent_file(std::path::Path::new(source))
                .await?
        };

        println!("Added torrent: {id}");
        Ok(())
    })
}

/// Run the list torrents command (CLI mode).
fn run_list_torrents(cfg: config::HibikiConfig) -> anyhow::Result<()> {
    let client = TorrentClient::new(&cfg.torrent);
    let torrents = client.list_torrents();

    if torrents.is_empty() {
        println!("No active torrents.");
    } else {
        for t in &torrents {
            println!(
                "[{}] {} - {:.0}% - {} - {} peers",
                t.id,
                t.name,
                t.progress * 100.0,
                t.state,
                t.peers
            );
        }
    }

    Ok(())
}

/// Run the daemon (background mode).
fn run_daemon(cfg: config::HibikiConfig) -> anyhow::Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    rt.block_on(async {
        let mut library = Library::new();
        let _torrent_client = TorrentClient::new(&cfg.torrent);

        // Initial library scan.
        match library.scan(&cfg.music_dir).await {
            Ok(count) => {
                tracing::info!(count, "initial library scan complete");
            }
            Err(e) => {
                tracing::error!("initial library scan failed: {e}");
            }
        }

        let stats = library.stats();
        tracing::info!(
            tracks = stats.total_tracks,
            artists = stats.total_artists,
            albums = stats.total_albums,
            "library indexed"
        );

        // In full daemon mode, we would:
        // 1. Start a Unix socket server (via tsunagu) for CLI-to-daemon IPC.
        // 2. Watch directories for new files (FSEvents/inotify).
        // 3. Manage the torrent client session.
        // 4. Respond to commands from the GUI or CLI.
        //
        // For now, just keep running.
        tracing::info!("daemon running. Press Ctrl+C to stop.");
        tokio::signal::ctrl_c().await?;
        tracing::info!("daemon shutting down");

        Ok(())
    })
}
