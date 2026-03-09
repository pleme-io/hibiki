# Hibikine (響音) -- GPU Music Player + BitTorrent

Binary: `hibiki` | Crate: `hibikine` | Config: `~/.config/hibiki/hibiki.yaml`

## Build & Test

```bash
cargo build                          # compile
cargo test --lib                     # unit tests (currently 30+)
cargo run                            # launch GUI (default: Play)
cargo run -- play /path/to/dir       # play a directory
cargo run -- daemon                  # background indexer + torrent client
cargo run -- add "magnet:?..."       # add torrent via daemon
cargo run -- torrents                # list active torrents
cargo run -- scan /path/to/music     # index music library
nix build                            # Nix build
nix run .#regenerate                 # regenerate Cargo.nix after dep changes
```

## Current State

Early scaffold. CLI dispatches to stub handlers. Core state machines exist but
lack real I/O integration:

- `audio.rs` -- `AudioEngine` with play/pause/stop/next/prev/volume/queue state
  machine. 17 tests. No actual rodio/symphonia I/O yet (TODOs in play/pause/stop).
- `library.rs` -- `Library` with async recursive scan, search, by_artist, by_album.
  17 tests. Metadata extraction is filename-based (TODO: symphonia probe).
- `torrent.rs` -- `TorrentClient` with add_magnet, add_torrent_file, list, completed.
  13 tests. No actual librqbit session (TODOs for session creation).
- `render.rs` -- Documentation-only module describing the planned GPU pipeline.
  No rendering code yet.
- `config.rs` -- `HibikiConfig` with shikumi discovery. Working.
- `main.rs` -- CLI via clap with Play/Daemon/Add/Torrents/Scan subcommands. Stubs only.

**NOTE:** Cargo.toml references `fude` and `kotoba` (old names). These have been
renamed to `mojiban` and `kaname` respectively. Update git URLs when implementing.

## Competitive Landscape

| Competitor | Stack | Strengths | Weaknesses vs hibiki |
|-----------|-------|-----------|---------------------|
| Strawberry | C++/Qt | Album art, equalizer, scrobbling, gapless, CD rip, subsonic/tidal | Qt dependency, no GPU rendering, no BitTorrent, no scripting |
| DeaDBeeF | C | 100+ plugins, customizable UI, scriptable, multi-format | C plugin API, no GPU rendering, no BitTorrent |
| cmus | C | TUI, vim-like, fast, lightweight, output plugins | Text-only, no visualizer, no BitTorrent, no plugin system |
| ncmpcpp | C++ | MPD client, TUI, vim-like, visualizer | MPD dependency, text-only, no BitTorrent |
| Spotify | Electron | Streaming, algorithmic playlists, social, discovery | Subscription, no local files, no FLAC, Electron bloat |
| Tidal | Electron | Hi-fi streaming, MQA/FLAC, curated playlists | Subscription, no local files, Electron bloat |

**Key differentiators:**
- GPU-rendered via garasu (not Qt/GTK/Electron)
- Built-in BitTorrent for legal music acquisition (Bandcamp, free releases, CC-licensed)
- MCP server for AI-assisted music workflows
- Rhai scripting (not C plugins or JavaScript)
- Nix-configured, declarative, hot-reloadable

## Architecture

### Data Flow

```
                    +-----------+
                    |  librqbit |
                    |  session  |
                    +-----+-----+
                          |  completed downloads
                          v
  ~/.config/hibiki/ --> Library Scanner --> Track Index (in-memory / sled)
                          |
                          v
  AudioEngine (oto) <-- Queue Manager --> Now Playing state
       |                                       |
       v                                       v
  rodio Sink <-- symphonia Decoder      GPU Renderer (garasu/madori)
       |                                       |
       v                                       v
  Audio Device                          winit Window (Metal/Vulkan)
```

### Module Map

| Module | Responsibility | Key Types | pleme-io Deps |
|--------|---------------|-----------|---------------|
| `audio` | Playback state machine, codec decoding, device output | `AudioEngine`, `Track`, `PlaybackState` | oto |
| `library` | Directory scanning, metadata extraction, search index | `Library`, `extract_metadata` | -- |
| `torrent` | BitTorrent downloads, magnet/file handling, completion tracking | `TorrentClient`, `TorrentStatus`, `TorrentState` | -- |
| `render` | GPU rendering pipeline, UI layout, visualizer | (planned) | garasu, madori, egaku, mojiban, irodzuki |
| `config` | Config struct, shikumi integration, hot-reload | `HibikiConfig`, `AudioConfig`, `TorrentConfig` | shikumi |
| `mcp` | (planned) MCP server for external automation | -- | kaname |
| `daemon` | (planned) Background process for indexing + torrents | -- | tsunagu |
| `plugin` | (planned) Rhai scripting engine | -- | soushi |

### Planned Source Layout

```
src/
  main.rs           # CLI entry point (clap)
  config.rs         # HibikiConfig + shikumi
  audio.rs          # AudioEngine, playback, codec detection
  library.rs        # Library scanner, metadata, search
  torrent.rs        # TorrentClient, magnet/file handling
  render/
    mod.rs          # Renderer orchestration
    player.rs       # Now-playing panel (art, controls, progress)
    library_view.rs # Library browser (artist/album/track lists)
    queue.rs        # Queue panel
    visualizer.rs   # Waveform/spectrum shader
    torrent_view.rs # Download progress panel
  mcp.rs            # MCP server (kaname)
  daemon.rs         # Background mode (tsunagu)
  plugin.rs         # Rhai scripting (soushi)
```

## pleme-io Library Integration

| Library | Role in hibiki |
|---------|---------------|
| **shikumi** | Config discovery + hot-reload for `HibikiConfig` |
| **garasu** | GPU context, text rendering, shader pipeline for visualizer |
| **madori** | App framework: event loop, render loop, input dispatch |
| **egaku** | Widgets: track list, queue, volume slider, progress bar, tabs |
| **irodzuki** | Base16 theme to GPU uniforms, ANSI colors for terminal output |
| **mojiban** | Rich text for track metadata, lyrics display |
| **oto** | Audio state machines: Player, Queue, Decoder, VoiceStream |
| **kaname** | MCP server scaffold for automation tools |
| **soushi** | Rhai scripting engine for user plugins |
| **awase** | Hotkey registration and parsing |
| **hasami** | Clipboard for copying track info, lyrics |
| **tsunagu** | Daemon lifecycle for background torrent + indexer service |
| **tsuuchi** | Notifications for download complete, track change |
| **todoku** | HTTP client for metadata API lookups (MusicBrainz, Last.fm) |

## Implementation Phases

### Phase 1: Audio Playback (current priority)
Wire `AudioEngine` to real rodio/symphonia I/O:
1. Open audio file with symphonia `Probe`
2. Create symphonia `FormatReader` + `Decoder`
3. Feed decoded samples to rodio `OutputStream` / `Sink`
4. Implement gapless crossfade between tracks
5. Extract real metadata (duration, sample rate, bit depth, tags) via symphonia probe
6. Replace filename-based `extract_metadata` with proper tag reading (ID3v2, Vorbis, FLAC)

### Phase 2: GUI Rendering
Build the GPU UI via madori + garasu:
1. Create `App::builder()` with madori, set up event loop
2. Implement `RenderCallback` for the player UI
3. Layout: left panel (library browser), center (now playing + art), right (queue)
4. Bottom bar: playback controls, progress bar, volume
5. Album art: decode embedded art or companion files, upload as wgpu texture
6. Track list: egaku `ListView` with mojiban rich text for metadata

### Phase 3: Visualizer
Real-time audio visualization via custom WGSL shaders:
1. Capture PCM samples from audio engine (ring buffer)
2. FFT for spectrum analysis (realfft crate)
3. Upload frequency/waveform data as wgpu uniform buffer
4. Render via garasu `ShaderPipeline` with custom WGSL
5. User-selectable modes: waveform, spectrum bars, circular, none

### Phase 4: BitTorrent Integration
Wire `TorrentClient` to librqbit:
1. Create `librqbit::Session` with configured download_dir
2. Add magnet links and .torrent files to real session
3. Progress reporting via `librqbit` session status
4. Auto-import: watch completed downloads, trigger library scan
5. Seeding management: configurable ratio limits, upload speed caps

### Phase 5: Daemon Mode
Background service via tsunagu:
1. `DaemonProcess` for PID file management
2. Unix socket for CLI-to-daemon communication
3. Background library indexer (inotify/FSEvents watch)
4. Background torrent client (persistent sessions)
5. Health check endpoint

### Phase 6: MCP Server
Embedded MCP server via kaname (stdio transport):
1. Standard tools: `status`, `config_get`, `config_set`, `version`
2. Playback tools: `play`, `pause`, `next`, `prev`, `seek`, `set_volume`, `get_now_playing`
3. Queue tools: `queue_add`, `queue_list`, `queue_clear`, `queue_reorder`
4. Library tools: `search_library`, `library_scan`, `library_stats`
5. Torrent tools: `torrent_add`, `torrent_status`, `torrent_list`, `torrent_remove`
6. Equalizer tools: `equalizer_get`, `equalizer_set`, `equalizer_presets`

### Phase 7: Plugin System
Rhai scripting via soushi:
1. Script loading from `~/.config/hibiki/scripts/*.rhai`
2. Rhai API: `hibiki.play()`, `hibiki.pause()`, `hibiki.next()`, `hibiki.prev()`,
   `hibiki.queue(path)`, `hibiki.volume(n)`, `hibiki.seek(seconds)`,
   `hibiki.search(query)`, `hibiki.torrent_add(magnet)`, `hibiki.visualizer(type)`,
   `hibiki.equalizer({...})`
3. Event hooks: `on_track_change`, `on_download_complete`, `on_library_update`
4. Custom command registration for command palette
5. Plugin manifest: `plugin.toml`

## Hotkey System

Modal keybindings via awase. Modes:

**Normal mode (default):**
| Key | Action |
|-----|--------|
| `Space` | Play/pause |
| `n` / `p` | Next / previous track |
| `+` / `-` | Volume up / down |
| `s` | Toggle shuffle |
| `r` | Cycle repeat (off / track / queue) |
| `m` | Mute/unmute |
| `/` | Focus search |
| `Tab` | Cycle panels (library / player / queue / torrent) |
| `1`-`4` | Jump to panel by number |
| `q` | Quit |
| `:` | Enter command mode |

**Library mode (when library panel focused):**
| Key | Action |
|-----|--------|
| `j` / `k` | Navigate up/down |
| `Enter` | Play selected |
| `a` | Add to queue |
| `t` | Toggle tag filter |
| `/` | Search within library |
| `Esc` | Back to normal |

**Queue mode (when queue panel focused):**
| Key | Action |
|-----|--------|
| `j` / `k` | Navigate |
| `d` | Remove from queue |
| `J` / `K` | Move item down/up |
| `c` | Clear queue |
| `Esc` | Back to normal |

**Command mode:**
`:play`, `:pause`, `:next`, `:prev`, `:add <path>`, `:torrent <magnet>`,
`:eq <preset>`, `:scan`, `:vol <0-100>`, `:seek <mm:ss>`, `:quit`

## Configuration

### Config Struct Hierarchy

```yaml
# ~/.config/hibiki/hibiki.yaml
music_dir: ~/Music
audio:
  sample_rate: 44100
  buffer_size: 4096
  gapless: true
  output_device: null        # null = system default
  equalizer:
    enabled: false
    preset: flat             # flat, rock, jazz, classical, bass_boost, custom
    bands: []                # custom EQ bands [{ freq_hz, gain_db }]
torrent:
  download_dir: ~/Music/Downloads
  max_connections: 50
  dht_enabled: true
  max_upload_kbps: 0         # 0 = unlimited
  seed_ratio_limit: 2.0
  auto_import: true          # auto-scan completed downloads
appearance:
  background: "#2e3440"
  foreground: "#eceff4"
  accent: "#88c0d0"
  visualizer: spectrum       # waveform, spectrum, circular, none
library:
  scan_on_startup: true
  watch_dirs: true           # FSEvents/inotify watch for new files
  metadata_source: tags      # tags, filename, both
keybindings: {}              # override default keybindings
```

### Env Overrides

- `HIBIKI_CONFIG=/path/to/config.yaml` -- full config path override
- `HIBIKI_MUSIC_DIR=~/Music` -- individual field override
- `HIBIKI_AUDIO__SAMPLE_RATE=48000` -- nested field (double underscore)
- `HIBIKI_TORRENT__DHT_ENABLED=false` -- nested field

## Nix Integration

### Flake Structure

The flake uses `rustPlatform.buildRustPackage` directly (not substrate
`rust-tool-release-flake.nix` yet). Consider migrating to substrate pattern
for multi-platform support and standardized apps.

**Exports:**
- `packages.aarch64-darwin.{hibiki,default}` -- the binary
- `overlays.default` -- `pkgs.hibiki`
- `homeManagerModules.default` -- HM module (needs creation)
- `devShells.aarch64-darwin.default` -- dev environment

### HM Module (to be created at `module/default.nix`)

The hibiki flake already wires `homeManagerModules.default` but the
`module/` directory does not exist yet. Create it following the shashin/hikki
pattern with these typed options:

- `blackmatter.components.hibiki.enable`
- `blackmatter.components.hibiki.package`
- `blackmatter.components.hibiki.music_dir`
- `blackmatter.components.hibiki.audio.{sample_rate, buffer_size, gapless}`
- `blackmatter.components.hibiki.torrent.{download_dir, max_connections, dht_enabled}`
- `blackmatter.components.hibiki.appearance.{background, foreground, accent}`
- `blackmatter.components.hibiki.daemon.enable` -- launchd/systemd service
- `blackmatter.components.hibiki.extraSettings`

Use substrate `hm-service-helpers.nix` for `mkLaunchdService`/`mkSystemdService`.

## Design Decisions

### Audio Stack
- **symphonia over ffmpeg**: Pure Rust, no C dependencies, broad codec support
  (FLAC, ALAC, WAV, AIFF, OGG, MP3, AAC, Opus). Nix-friendly (no bindgen).
- **rodio for output**: Cross-platform audio output, integrates with symphonia.
  Handles device enumeration and sample rate conversion.
- **oto for state machines**: pleme-io shared library provides Player/Queue state
  machines. Wire rodio/symphonia as the I/O backend behind oto's trait interfaces.

### BitTorrent Stack
- **librqbit over libtorrent**: Pure Rust, async, no C++ binding. Supports DHT,
  peer exchange, magnet links, selective download.
- **Daemon mode**: Torrent client persists in background via tsunagu. CLI and GUI
  both communicate with daemon over Unix socket.
- **Auto-import**: Completed downloads trigger automatic library scan of the output
  directory. Configurable via `torrent.auto_import`.

### Library Management
- **In-memory index first**: Start with `Vec<Track>` search. Migrate to sled/tantivy
  when library size warrants it (>10K tracks).
- **Metadata extraction**: symphonia probe for audio tags. Fall back to filename
  parsing ("Artist - Title.flac") when tags are missing.
- **Album art**: Extract from ID3 APIC frames / Vorbis METADATA_BLOCK_PICTURE.
  Fall back to companion files (cover.jpg, folder.png, front.png) in same directory.

### GPU Rendering
- **madori for app shell**: Event loop, render loop, input dispatch. Eliminates
  ~200 lines of boilerplate vs raw garasu + winit.
- **Visualizer via garasu ShaderPipeline**: Custom WGSL shaders receive FFT data
  as uniform buffer. Hot-reloadable from `~/.config/hibiki/shaders/`.
- **Layout**: Three-panel layout (library | player | queue) with egaku SplitPane.
  Bottom bar for controls. Responsive resize via egaku layout system.

## Testing Strategy

- **Unit tests**: State machine logic (audio, library, torrent) with no I/O.
  Already 30+ tests. Target 80%+ coverage on state machines.
- **Integration tests**: Requires real audio files. Use `tests/fixtures/` with
  small WAV files for decode/playback testing.
- **Visual tests**: GPU rendering tested via screenshot comparison (garasu test utilities).
- **Torrent tests**: Mock librqbit session for unit tests. Integration tests use
  local tracker + seeder.

## Error Handling

- `AudioError`, `LibraryError`, `TorrentError` -- module-specific error enums
  with `thiserror`. Each wraps `std::io::Error` via `#[from]`.
- Top-level uses `anyhow::Result` for CLI error reporting.
- Library code returns typed errors. Never panic in library code.
- Tracing for all operations: `tracing::{info,debug,warn,error}` with structured fields.
