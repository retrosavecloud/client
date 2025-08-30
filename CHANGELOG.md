# Changelog

All notable changes to RetroSave Client will be documented in this file.

## [0.1.0] - 2025-08-30

### 🎉 Initial MVP Release

This is the first public release of RetroSave - a cloud save management system for retro game emulators.

### Features
- 🎮 **PCSX2 Support** - Automatic detection and monitoring of PCSX2 emulator
- 💾 **Save Detection** - Monitors save directories for changes
- 🗜️ **Compression** - Zstd compression for efficient storage
- 🔒 **Local Storage** - SQLite database for save management
- 🖥️ **System Tray** - Minimal UI with system tray integration
- ⚙️ **Settings** - Configurable monitoring intervals and paths

### Technical Details
- Built with Rust for performance and reliability
- Cross-platform support (Windows, Linux, macOS)
- Event-driven architecture
- Automatic save versioning (keeps last 5 versions)

### Known Limitations
- Cloud sync coming in v0.2.0
- Only PCSX2 support (more emulators coming)
- Manual configuration required for custom save paths

### Installation
Download the appropriate package for your platform from the [releases page](https://github.com/retrosavecloud/client/releases).

---

For more information, visit [retrosave.cloud](https://retrosave.cloud)