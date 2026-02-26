# Install Simplex (GTK Native)

## Linux

### Option 1: Use a release artifact

Download the latest Linux artifacts from GitHub Releases:

- `simplex-linux-x86_64` (raw binary)
- `simplex_<version>_amd64.deb` (Debian/Ubuntu package)
- `SHA256SUMS.txt` (checksum verification)

### Option 2: Build from source

Ubuntu/Debian:

```bash
sudo apt install \
  libgtk-4-dev libadwaita-1-dev \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
  libgstreamer-plugins-bad1.0-dev \
  gstreamer1.0-plugins-good gstreamer1.0-plugins-bad \
  gstreamer1.0-plugins-ugly gstreamer1.0-vaapi gstreamer1.0-libav
```

Fedora:

```bash
sudo dnf install \
  gtk4-devel libadwaita-devel \
  gstreamer1-devel gstreamer1-plugins-base-devel \
  gstreamer1-plugins-bad-free-devel \
  gstreamer1-plugins-good gstreamer1-plugins-bad-free \
  gstreamer1-plugins-ugly-free gstreamer1-vaapi gstreamer1-libav
```

Then:

```bash
cargo run -p simplex-app
```

## Windows

### Option 1: Use a release artifact

Download from GitHub Releases:

- `simplex-windows-x86_64.exe`
- `SHA256SUMS.txt`

### Option 2: Build from source (MSYS2 + Rust)

Install [MSYS2](https://www.msys2.org/) and run:

```bash
pacman -S mingw-w64-ucrt-x86_64-gtk4 mingw-w64-ucrt-x86_64-libadwaita \
  mingw-w64-ucrt-x86_64-gstreamer mingw-w64-ucrt-x86_64-gst-plugins-base \
  mingw-w64-ucrt-x86_64-gst-plugins-good mingw-w64-ucrt-x86_64-gst-plugins-bad \
  mingw-w64-ucrt-x86_64-pkg-config
```

Build:

```bash
cargo build --release -p simplex-app
```

## Verify Downloads

Linux:

```bash
sha256sum -c SHA256SUMS.txt
```

Windows PowerShell:

```powershell
Get-FileHash .\simplex-windows-x86_64.exe -Algorithm SHA256
```

## Uninstall

- Linux `.deb`: `sudo apt remove simplex`
- Linux binary: delete the binary and desktop entry you installed
- Windows: remove executable and any shortcuts
