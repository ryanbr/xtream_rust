# Xtreme IPTV Player - Rust Edition

A fast, lightweight, cross-platform IPTV player with Xtream Codes API support built in Rust.

![Platform](https://img.shields.io/badge/platform-Windows%20|%20Linux%20|%20macOS-blue)
![Language](https://img.shields.io/badge/language-Rust-orange)
![License](https://img.shields.io/badge/license-MIT-green)

## Features

- 🔐 **Xtream Codes API** - Full support for login, live TV, movies, and series
- 📺 **Live TV** - Browse categories and play live streams
- 🎬 **Movies & Series** - Browse VOD content with seasons/episodes
- ⭐ **Favorites** - Star your favorite channels for quick access
- 📚 **Address Book** - Save multiple server credentials
- 🔍 **Search** - Filter channels and content
- 🎨 **Dark/Light Mode** - Toggle UI theme
- 🎮 **Hardware Acceleration** - GPU-accelerated video decoding
- 🌐 **User Agent Spoofing** - 35+ preset user agents
- 📶 **Connection Quality Presets** - Optimized buffering for your connection
- 🖥️ **Multi-Player Support** - VLC, mpv, ffplay, and more
- 💾 **Save State** - Remember login and settings
- 📋 **M3U Support** - Parse and play M3U/M3U8 playlists
- 🖱️ **Single Window Mode** - Auto-close previous player

## Screenshots

```
┌─────────────────────────────────────────────────────────────────┐
│  Xtreme IPTV Player - Rust Edition                              │
├─────────────────────────────────────────────────────────────────┤
│  Server: [http://example.com:8080    ] [Login] [📚 Address Book]│
│  User:   [username] Pass: [••••••••]                            │
├─────────────────────────────────────────────────────────────────┤
│  🎬 Player: [vlc        ] [📁] | 📶 Connection: [Normal] (5s)   │
│  ☑ HW Acceleration                                              │
├─────────────────────────────────────────────────────────────────┤
│  [📺 Live TV]  [🎬 Movies]  [📺 Series]  [⭐ Favorites]         │
├─────────────────────────────────────────────────────────────────┤
│  Categories          │  Channels                                │
│  ──────────────────  │  ────────────────────────────────────    │
│  > Sports            │  ⭐ ESPN HD                              │
│  > News              │  ⭐ CNN International                    │
│  > Entertainment     │     BBC World News                       │
│  > Kids              │     Discovery Channel                    │
│  > Movies            │     National Geographic                  │
└─────────────────────────────────────────────────────────────────┘
```

## Comparison: Rust vs Other IPTV Players

| Feature | Xtreme IPTV (Rust) | Typical Electron App | Native C++ App |
|---------|-------------------|---------------------|----------------|
| **Binary Size** | ~4 MB | ~150+ MB | ~10-20 MB |
| **RAM Usage** | ~30-50 MB | ~200-500 MB | ~50-100 MB |
| **Startup Time** | <1 second | 3-5 seconds | 1-2 seconds |
| **CPU Idle** | <1% | 2-5% | 1-2% |
| **Dependencies** | None (standalone) | Node.js, Chromium | Various libs |
| **Cross-platform** | ✅ Single codebase | ✅ Single codebase | ❌ Per-platform |
| **Memory Safety** | ✅ Guaranteed | ✅ (JS runtime) | ❌ Manual |
| **HW Acceleration** | ✅ GPU decoding | ⚠️ Varies | ✅ GPU decoding |
| **No Console Window** | ✅ Windows | ✅ | ⚠️ Varies |

## Supported Platforms

| Platform | Architecture | Status | Binary |
|----------|--------------|--------|--------|
| Windows | x64 (Intel/AMD) | ✅ Optimized | `xtreme_iptv_windows_x64.exe` |
| Windows | ARM64 (Snapdragon) | ✅ Optimized | `xtreme_iptv_windows_arm64.exe` |
| Linux | x64 (Intel/AMD) | ✅ Optimized | `xtreme_iptv_linux_x64` |
| macOS | x64 / ARM64 | ✅ Supported | Build from source |

### CPU Optimizations

| Platform | Optimizations |
|----------|---------------|
| **Windows/Linux x64** | AVX, AVX2, BMI1, BMI2, FMA, LZCNT, POPCNT (x86-64-v3) |
| **Windows ARM64** | NEON, AES, SHA2, CRC32, LSE, FP16, DotProd (Snapdragon optimized) |

## Installation

### Pre-built Binaries

Download from the [Releases](https://github.com/yourrepo/xtreme_iptv/releases) page.

### Build from Source

#### Prerequisites

- [Rust](https://rustup.rs/) 1.70+
- For Windows cross-compile: `mingw-w64`
- For Windows ARM64 cross-compile: `llvm-mingw`

#### Build Commands

```bash
# Linux x64
./build.sh linux

# Windows x64 (cross-compile from Linux)
./build.sh windows

# Windows ARM64 (cross-compile from Linux)
./build.sh windows-arm

# All platforms
./build.sh everything

# Show help
./build.sh help
```

#### Install Dependencies (Linux)

```bash
# Ubuntu/Debian
sudo apt install mingw-w64

# Fedora
sudo dnf install mingw64-gcc

# Arch
sudo pacman -S mingw-w64-gcc
```

## Usage

### Quick Start

1. Launch the application
2. Enter your Xtream Codes server details:
   - Server: `http://yourserver.com:port`
   - Username: your username
   - Password: your password
3. Click **Login**
4. Browse Live TV, Movies, or Series
5. Double-click a channel to play

### Player Configuration

Enter your preferred media player in the **Player** field:

| Player | Value | Notes |
|--------|-------|-------|
| VLC | `vlc` | Auto-detected on Windows |
| mpv | `mpv` | Recommended, best performance |
| ffplay | `ffplay` or leave empty | Default player |
| Custom | Full path | e.g., `C:\Program Files\VLC\vlc.exe` |

### Connection Quality Presets

| Preset | Buffer | Best For |
|--------|--------|----------|
| ⚡ Fast | 2s | Fiber, high-speed connections |
| 📶 Normal | 5s | Standard broadband |
| 🐢 Slow | 15s | DSL, congested networks |
| 🦥 Very Slow | 30s | Mobile, satellite, poor connections |
| ⚙️ Custom | 1-120s | Manual configuration |

### Hardware Acceleration

Enable **HW Acceleration** checkbox to use GPU video decoding:

| Platform | Decoder |
|----------|---------|
| Windows | DXVA2 / D3D11VA |
| Linux | VA-API / VDPAU |
| macOS | VideoToolbox |

Disable if you experience playback issues with certain streams.

### Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `Enter` | Play selected channel |
| `Escape` | Go back |
| `Ctrl+F` | Focus search |
| `Ctrl+S` | Save settings |

## Configuration

Settings are stored in:

| Platform | Location |
|----------|----------|
| Windows | `%APPDATA%\xtreme_iptv\config.json` |
| Linux | `~/.config/xtreme_iptv/config.json` |
| macOS | `~/Library/Application Support/xtreme_iptv/config.json` |

### Config Options

```json
{
  "external_player": "vlc",
  "buffer_seconds": 5,
  "connection_quality": "Normal",
  "dark_mode": true,
  "hw_accel": true,
  "single_window_mode": true,
  "save_state": true,
  "pass_user_agent_to_player": true
}
```

## Troubleshooting

### VLC won't start
- Use full path: `C:\Program Files\VideoLAN\VLC\vlc.exe`
- Or add VLC to system PATH

### Video buffering/stuttering
1. Increase buffer: Change **Connection** to **Slow** or **Very Slow**
2. Try different player (mpv often performs better)
3. Disable HW Acceleration if GPU issues

### Hardware acceleration errors
```
hardware acceleration picture allocation failed
```
- Uncheck **HW Acceleration** to use CPU decoding
- Update GPU drivers

### Stream won't play
- Check User Agent settings
- Try different User Agent preset
- Verify stream URL works in browser

## Building with Internal Player (Optional)

The internal FFmpeg player is optional and requires FFmpeg development libraries:

```bash
# Install FFmpeg dev libs (Linux)
sudo apt install libavcodec-dev libavformat-dev libavutil-dev libswscale-dev pkg-config clang

# Build with internal player
./build.sh linux --internal-player
```

Note: Internal player is video-only (no audio) and mainly for testing.

## License

MIT License - See [LICENSE](LICENSE) for details.

## Credits

- Built with [egui](https://github.com/emilk/egui) - Immediate mode GUI
- [eframe](https://github.com/emilk/egui/tree/master/crates/eframe) - Native framework
- Rust community for excellent crates

## Contributing

Contributions welcome! Please open an issue or PR.

---

**Disclaimer:** This software is for personal use with legally obtained IPTV subscriptions only. The developers are not responsible for misuse.
