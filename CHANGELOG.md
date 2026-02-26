# Changelog

All notable changes to this project will be documented in this file.

## [0.1.3] - 2026-02-25

### Added

- Persistent mini-player for collapsed playback controls while browsing
- Artist detail layout improvements (Popular, Albums, EPs/Singles)
- Universal drill-in links for key media entities
- L1+L2 local cache layer with endpoint-aware TTL behavior

### Changed

- Distinct library rendering styles for movie/show/music sections
- Playback progress sync and completion state integration with Plex timeline/scrobble APIs
- CI/release workflows aligned to GTK native app artifacts with checksum outputs

### Security

- Config serialization now skips auth token persistence
- Restrictive cache file permissions on Unix
- Reduced panic output sensitivity for release builds
