# Simplex

A native desktop media client for Plex servers, built with GTK4 + GStreamer in Rust.

**This project is not affiliated with Plex, Inc.**

## Features

- **Hardware-accelerated video playback** via GStreamer (VA-API on Linux, D3D11VA on Windows)
- **Picture-in-Picture** with subtitle overlay
- **Audio track change detection** -- pauses playback when the audio language changes unexpectedly and no preferred language track is available
- **Custom library browser** -- browse libraries, search, playlists, collections, continue watching / on deck
- **OAuth authentication** via PIN-based flow through the system browser
- **User switching** between home users
- **Deep link support** via `simplex://` protocol

## Architecture

```
simplex/
├── crates/
│   ├── simplex-core/     # Platform-agnostic: Plex API, config, auth, media logic
│   └── simplex-app/      # GTK4 + GStreamer UI (Linux + Windows)
└── Cargo.toml            # Workspace root
```

The `simplex-core` crate contains all business logic and can be shared with future platform backends (e.g., macOS via AppKit + AVFoundation).

## Building

### Prerequisites (Linux)

```bash
# Ubuntu/Debian
sudo apt install \
  libgtk-4-dev libadwaita-1-dev \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
  libgstreamer-plugins-bad1.0-dev \
  gstreamer1.0-plugins-good gstreamer1.0-plugins-bad \
  gstreamer1.0-plugins-ugly gstreamer1.0-vaapi gstreamer1.0-libav

# Fedora
sudo dnf install \
  gtk4-devel libadwaita-devel \
  gstreamer1-devel gstreamer1-plugins-base-devel \
  gstreamer1-plugins-bad-free-devel \
  gstreamer1-plugins-good gstreamer1-plugins-bad-free \
  gstreamer1-plugins-ugly-free gstreamer1-vaapi gstreamer1-libav
```

### Prerequisites (Windows)

Install [MSYS2](https://www.msys2.org/) and run:

```bash
pacman -S mingw-w64-ucrt-x86_64-gtk4 mingw-w64-ucrt-x86_64-libadwaita \
  mingw-w64-ucrt-x86_64-gstreamer mingw-w64-ucrt-x86_64-gst-plugins-base \
  mingw-w64-ucrt-x86_64-gst-plugins-good mingw-w64-ucrt-x86_64-gst-plugins-bad \
  mingw-w64-ucrt-x86_64-pkg-config
```

### Build & Run

```bash
# Run tests
cargo test -p simplex-core

# Build and run
cargo run -p simplex-app
```

## Configuration

Config is stored at:
- **Linux**: `~/.config/simplex/config.json`
- **Windows**: `%APPDATA%/simplex/config.json`

Auth tokens are stored in the OS keychain (libsecret on Linux, Windows Credential Manager on Windows).

## Protocol Handler (Linux)

```bash
cp simplex.desktop ~/.local/share/applications/
update-desktop-database ~/.local/share/applications/
xdg-mime default simplex.desktop x-scheme-handler/simplex
```

## Deep Link Formats

- `simplex://server/{serverId}/details?key=/library/metadata/123`
- `simplex://open?baseUrl=http%3A%2F%2Flocalhost%3A32400&key=%2Flibrary%2Fmetadata%2F123`
- `simplex://open?key=/library/metadata/456`

## License

[MIT License](LICENSE.md)
