//! Hibiki (響) — GPU-rendered music player with built-in BitTorrent.
//!
//! A high-fidelity music player that combines:
//! - GPU-accelerated UI via garasu (wgpu/winit)
//! - Lossless audio playback via rodio + symphonia (FLAC, ALAC, WAV, etc.)
//! - Built-in BitTorrent client for discovering and downloading music
//! - Library management with metadata extraction
//! - Hot-reloadable configuration via shikumi

mod audio;
mod config;
mod library;
mod render;
mod torrent;

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

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
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    let config = config::load(&cli.config)?;

    match cli.command {
        None | Some(Commands::Play { .. }) => {
            tracing::info!("launching hibiki player");
            // TODO: Initialize garasu GPU context
            // TODO: Create winit window
            // TODO: Enter render loop with audio playback
        }
        Some(Commands::Daemon) => {
            tracing::info!("starting hibiki daemon");
            // TODO: Start library indexer + torrent client daemon via tsunagu
        }
        Some(Commands::Add { source }) => {
            tracing::info!("adding torrent: {source}");
            // TODO: Connect to daemon, add torrent
        }
        Some(Commands::Torrents) => {
            // TODO: Connect to daemon, list torrents
        }
        Some(Commands::Scan { path }) => {
            tracing::info!("scanning library");
            // TODO: Scan directory, extract metadata, index
        }
    }

    Ok(())
}
