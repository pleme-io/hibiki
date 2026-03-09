# Hibiki (響)

GPU-rendered music player with built-in BitTorrent client for hi-fi music.

## Features

- GPU-accelerated UI via garasu (wgpu Metal/Vulkan)
- Hi-fi audio: FLAC, ALAC, WAV, AIFF, OGG, MP3, AAC (via symphonia)
- Gapless playback
- Built-in BitTorrent client (magnet links, .torrent files, DHT)
- Library management with metadata scanning
- Album art display and waveform visualizer
- Hot-reloadable configuration via shikumi

## Architecture

| Module | Purpose |
|--------|---------|
| `audio` | rodio + symphonia playback engine |
| `torrent` | librqbit BitTorrent client |
| `library` | Music scanning, metadata extraction, indexing |
| `render` | GPU UI via garasu |
| `config` | shikumi-based configuration |

## Dependencies

- **garasu** — GPU rendering engine
- **tsunagu** — daemon IPC (background indexer + torrent client)
- **shikumi** — config discovery + hot-reload

## Build

```bash
cargo build
cargo run
cargo run -- daemon
cargo run -- add "magnet:?xt=..."
cargo run -- scan ~/Music
```

## Configuration

`~/.config/hibiki/hibiki.yaml`

```yaml
music_dir: ~/Music
audio:
  sample_rate: 44100
  buffer_size: 4096
  gapless: true
torrent:
  download_dir: ~/Music/Downloads
  max_connections: 50
  dht_enabled: true
```
