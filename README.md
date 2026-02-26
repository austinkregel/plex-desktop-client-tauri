# Simplex

Simplex is a GTK native desktop client for Plex servers, built in Rust with GTK4 + GStreamer.

This project is not affiliated with Plex, Inc.

## Highlights

- Native GTK UI for desktop browsing and playback
- Hardware-accelerated playback with GStreamer
- Library, search, playlists, collections, and detail drill-in
- OAuth sign-in via browser + system keychain token storage
- Persistent mini-player for audio/video while browsing

## Supported Platforms

- Linux (primary)
- Windows (supported)
- macOS (not officially supported yet)

## Quick Links

- Install and build instructions: [`docs/INSTALL.md`](docs/INSTALL.md)
- First-run setup: [`docs/GETTING_STARTED.md`](docs/GETTING_STARTED.md)
- Troubleshooting: [`docs/TROUBLESHOOTING.md`](docs/TROUBLESHOOTING.md)
- Known limitations: [`docs/LIMITATIONS.md`](docs/LIMITATIONS.md)
- Changelog: [`CHANGELOG.md`](CHANGELOG.md)

## Configuration And Security

- Config file:
  - Linux: `~/.config/simplex/config.json`
  - Windows: `%APPDATA%/simplex/config.json`
- Auth tokens are stored in the OS keychain, not in plaintext config.

## Development

```bash
cargo test --workspace
cargo run -p simplex-app
```

## License

[MIT License](LICENSE.md)
