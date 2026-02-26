# Troubleshooting

## Sign-In Issues

- **Browser opens but sign-in never completes**
  - Confirm system clock is correct.
  - Disable VPN/proxy temporarily and retry.
  - Try again from the login page after waiting 10-15 seconds.

- **"Authentication timed out"**
  - Restart sign-in and complete browser approval promptly.

## Server Connection Issues

- Confirm Plex Media Server is reachable from this machine.
- Verify local firewall allows access to Plex server port (typically `32400`).
- Ensure the selected server URL uses `http://` or `https://`.

## Playback Issues

- **Black screen or no playback**
  - Check GStreamer plugins are installed (`good`, `bad`, `ugly`, `libav`).
  - Try another media item to isolate file-specific issues.

- **No audio controls visible while browsing**
  - Start playback from detail and collapse player (Escape/back).
  - The mini-player appears only when an active playback session exists.

## Linux Dependency Checklist

Install required GTK/GStreamer packages from [`INSTALL.md`](INSTALL.md).

## Log And Config Locations

- Config:
  - Linux: `~/.config/simplex/config.json`
  - Windows: `%APPDATA%/simplex/config.json`
- Cache:
  - Linux: `~/.cache/simplex/api-cache`

If reporting a bug, include:
- OS version
- how to reproduce
- expected behavior vs actual behavior
