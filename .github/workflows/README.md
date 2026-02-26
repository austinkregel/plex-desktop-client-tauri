# GitHub Actions Workflows

This repository ships the GTK native app (`simplex-app`) as the public release artifact.

## Workflows

- `ci.yml`
  - Runs format/lint/tests for the Rust workspace.
  - Intended for fast quality gates on pushes and pull requests.

- `build.yml`
  - Builds release artifacts for GTK:
    - Linux x86_64 binary + `.deb`
    - Windows x86_64 binary
  - Generates `SHA256SUMS.txt` for each platform artifact set.
  - Publishes artifacts to GitHub Releases when tag matches `v*`.

## Creating A Release

1. Bump crate versions (`crates/simplex-app`, `crates/simplex-core`) and update changelog.
2. Tag and push:
   ```bash
   git tag -a v0.1.0 -m "Release v0.1.0"
   git push origin v0.1.0
   ```
3. Let `build.yml` complete and attach artifacts/checksums to the release.

## Release Assets

- Linux:
  - `simplex-linux-x86_64`
  - `simplex_<version>_amd64.deb`
  - `SHA256SUMS.txt`
- Windows:
  - `simplex-windows-x86_64.exe`
  - `SHA256SUMS.txt`

## Troubleshooting

- Build dependency failures:
  - Verify GTK4/libadwaita/GStreamer packages are present in workflow dependency install steps.
- Missing release artifacts:
  - Ensure tag starts with `v` and workflow has `contents: write` permission.
- Checksum mismatches:
  - Re-download artifact and recompute SHA256 locally before publishing.




