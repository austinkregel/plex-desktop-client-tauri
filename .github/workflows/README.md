# GitHub Actions Workflows

## Release Workflow

The `release.yml` workflow automatically builds and publishes your Tauri application when a GitHub release is published.

### How It Works

1. **Trigger**: The workflow runs when you publish a GitHub release (not a draft)
2. **Version Extraction**: Extracts the version number from the release tag (supports both `v1.0.0` and `1.0.0` formats)
3. **Version Sync**: Updates the version in:
   - `package.json`
   - `src-tauri/tauri.conf.json`
   - `src-tauri/Cargo.toml`
4. **Build**: Builds the app for all platforms in parallel:
   - Linux (x86_64): AppImage, .deb, .rpm
   - Windows (x86_64): .msi, .exe
   - macOS Intel (x86_64): .dmg
   - macOS Apple Silicon (ARM64): .dmg
5. **Upload**: Automatically uploads all build artifacts to the GitHub release

### Creating a Release

1. **Tag your release**:
   ```bash
   git tag -a v1.0.0 -m "Release version 1.0.0"
   git push origin v1.0.0
   ```

2. **Create a GitHub Release**:
   - Go to your repository on GitHub
   - Click "Releases" → "Draft a new release"
   - Select the tag you just created (or create a new tag)
   - Fill in the release title and description
   - Click "Publish release" (not "Save draft")

3. **Wait for builds**: The workflow will automatically:
   - Build for all platforms
   - Upload artifacts to the release
   - This typically takes 10-20 minutes

### Version Format

The workflow supports both tag formats:
- `v1.0.0` (recommended)
- `1.0.0`

The version will be extracted and used to update all version files automatically.

### Build Artifacts

After the workflow completes, you'll find these files attached to your release:

**Linux:**
- `plex-desktop_<version>_amd64.AppImage`
- `plex-desktop_<version>_amd64.deb`
- `plex-desktop_<version>_amd64.rpm`

**Windows:**
- `plex-desktop_<version>_x64-setup.exe` (NSIS installer)
- `plex-desktop_<version>_x64.msi` (MSI installer)

**macOS:**
- `plex-desktop_<version>_x64.dmg` (Intel)
- `plex-desktop_<version>_aarch64.dmg` (Apple Silicon)

### Troubleshooting

**Build fails:**
- Check the workflow logs in the "Actions" tab
- Ensure all dependencies are properly configured
- Verify the version format matches the expected pattern

**Artifacts not uploaded:**
- Ensure `GITHUB_TOKEN` has write permissions (this is automatic for public repos)
- Check that the release was published (not saved as draft)
- Verify the file paths match the actual build output locations

**Version not updating:**
- Check that the tag format is correct
- Verify the sed commands work on your platform (they're platform-specific)

### Customization

To modify which platforms are built or which artifacts are uploaded, edit `.github/workflows/release.yml`:

- Add/remove build jobs for different platforms
- Modify the `files` section in upload steps to include/exclude specific artifacts
- Adjust build arguments in the `npm run tauri build` commands

