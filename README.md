# Retrosave Client

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

Open-source client for Retrosave - automatic save management for retro game emulators.

## Features

- 🎮 Automatic save detection for multiple emulators
- 💾 Local save versioning and management
- ☁️ Cloud sync support (requires retrosave.cloud account)
- 🖥️ Cross-platform: Windows, Linux, Steam Deck
- 🔒 Privacy-first with optional client-side encryption

## Installation

See [Releases](https://github.com/retrosavecloud/client/releases) for pre-built binaries.

### Build from Source

```bash
# Copy environment configuration
cp .env.example .env
# Edit .env with your API URL (default: http://localhost:8080)

# Build and run
cargo build --release
./target/release/retrosave
```

⚠️ **Security Notice**: Never commit `.env` files to version control. They contain sensitive configuration that should remain private.

## Contributing

We welcome contributions! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

MIT License - see [LICENSE](LICENSE) for details.
