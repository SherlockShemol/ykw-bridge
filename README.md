<div align="center">

# You Know Who Bridge (YKW Bridge)

### Claude Code + Claude Desktop Provider Manager

[![Version](https://img.shields.io/github/v/release/SherlockShemol/ykw-bridge?color=blue&label=version)](https://github.com/SherlockShemol/ykw-bridge/releases)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](https://github.com/SherlockShemol/ykw-bridge/releases)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-orange.svg)](https://tauri.app/)
[![Downloads](https://img.shields.io/github/downloads/SherlockShemol/ykw-bridge/total)](https://github.com/SherlockShemol/ykw-bridge/releases/latest)

English | [中文](README_ZH.md) | [日本語](README_JA.md) | [Releases](https://github.com/SherlockShemol/ykw-bridge/releases)

</div>

## Acknowledgement

YKW Bridge started from the open-source project [**CC Switch**](https://github.com/farion1231/cc-switch).

We would like to thank the original author [**Jason Young**](https://github.com/farion1231) and all contributors for the excellent work they put into the original project. Their effort created the solid foundation that made this project possible.

Based on that foundation, we continue to modify, maintain, and publish this project under the **MIT License**, while retaining the relevant copyright and license notices. We respect the original project and deeply appreciate the open-source work behind it.

This standalone version exists for a simple reason: to better fit our own workflow, maintenance direction, and practical needs, while also sharing those changes with people who may have similar needs.

If you like this project, you are also welcome to check out the original project [CC Switch](https://github.com/farion1231/cc-switch) and the original author [Jason Young](https://github.com/farion1231).

This repository is independently maintained and continues on its own path.


## Why YKW Bridge?

Modern Claude workflows still mean juggling provider configs, proxy settings, MCP servers, prompts, skills, and Claude Desktop launch/runtime setup. Switching providers by hand usually means editing JSON and hoping nothing breaks.

**YKW Bridge** is now focused on **Claude Code** and **Claude Desktop**. It gives you one desktop app for Claude-side provider management, managed auth, MCP, prompts, skills, sessions, usage, proxy, failover, and Claude Desktop doctor/certificate/launch tooling.

- **One app for Claude workflows** — Manage Claude Code and Claude Desktop from a single interface
- **No More Manual Editing** — Import providers visually and switch quickly without hand-editing config files
- **Unified MCP & Skills Management** — One panel for Claude-side MCP servers and skills
- **System Tray Quick Switch** — Switch providers instantly from the tray menu, no need to open the full app
- **Cloud Sync** — Sync provider data across devices via Dropbox, OneDrive, iCloud, or WebDAV servers
- **Cross-Platform** — Native desktop app for Windows, macOS, and Linux, built with Tauri 2
- **Built-in Utilities** — Includes various utilities for first-launch login confirmation, signature bypass, plugin extension sync, and more

## Screenshots

|                  Main Interface                   |                  Add Provider                  |
| :-----------------------------------------------: | :--------------------------------------------: |
| ![Main Interface](assets/screenshots/main-en.png) | ![Add Provider](assets/screenshots/add-en.png) |

## Features

[GitHub Releases](https://github.com/SherlockShemol/ykw-bridge/releases)

### Provider Management

- **Claude provider management** — Import, edit, switch, sort, and export Claude providers
- **Managed auth preserved** — GitHub Copilot OAuth and ChatGPT / OpenAI OAuth still work inside the Claude workflow
- One-click switching, system tray quick access, drag-and-drop sorting, import/export

### Proxy & Failover

- **Local proxy with hot-switching** — Format conversion, auto-failover, circuit breaker, provider health monitoring, and request rectifier
- **App-level takeover** — Independently proxy Claude Code and Claude Desktop, down to individual providers

### MCP, Prompts & Skills

- **Unified MCP panel** — Manage Claude MCP servers with Deep Link import
- **Prompts** — Markdown editor for Claude prompt files with backfill protection
- **Skills** — One-click install from GitHub repos or ZIP files, custom repository management, with symlink and file copy support

### Usage & Cost Tracking

- **Usage dashboard** — Track spending, requests, and tokens with trend charts, detailed request logs, and custom per-model pricing

### Session Manager & Workspace

- Browse, search, and restore Claude conversation history
- **Claude Desktop tools** — Doctor, certificate, launch shim, watchdog, and takeover status in one place

### System & Platform

- **Cloud sync** — Custom config directory (Dropbox, OneDrive, iCloud, NAS) and WebDAV server sync
- **Deep Link** (`ykwbridge://`) — Import providers, MCP servers, prompts, and skills via URL
- Dark / Light / System theme, auto-launch, auto-updater, atomic writes, auto-backups, i18n (zh/en/ja)

## FAQ

<details>
<summary><strong>Which tools does YKW Bridge support now?</strong></summary>

This edition supports **Claude Code** and **Claude Desktop**. **GitHub Copilot** and **ChatGPT / OpenAI** auth remain available as managed auth inside the Claude provider flow.

</details>

<details>
<summary><strong>Do I need to restart the terminal after switching providers?</strong></summary>

For most tools, yes — restart your terminal or the CLI tool for changes to take effect. The exception is **Claude Code**, which currently supports hot-switching of provider data without a restart.

</details>

<details>
<summary><strong>My plugin configuration disappeared after switching providers — what happened?</strong></summary>

YKW Bridge provides a "Shared Config Snippet" feature to pass common data (beyond API keys and endpoints) between providers. Go to "Edit Provider" → "Shared Config Panel" → click "Extract from Current Provider" to save all common data. When creating a new provider, check "Write Shared Config" (enabled by default) to include plugin data in the new provider. All your configuration items are preserved in the default provider imported when you first launched the app.

</details>

<details>
<summary><strong>macOS installation</strong></summary>

YKW Bridge for macOS is code-signed and notarized by Apple. You can download and install it directly — no extra steps needed. We recommend using the `.dmg` installer.

</details>

<details>
<summary><strong>Why can't I delete the currently active provider?</strong></summary>

YKW Bridge follows a "minimal intrusion" design principle — even if you uninstall the app, Claude Code will continue to work normally. The system always keeps one active configuration, because deleting every configuration would leave Claude without a usable provider. To switch back to official login, see the next question.

</details>

<details>
<summary><strong>How do I switch back to official login?</strong></summary>

Add an official Claude provider from the preset list. After switching to it, run the Log out / Log in flow, and then you can freely switch between the official provider and third-party providers.

</details>

<details>
<summary><strong>Where is my data stored?</strong></summary>

- **Database**: `~/.ykw-bridge/ykw-bridge.db` (SQLite — providers, MCP, prompts, skills)
- **Local settings**: `~/.ykw-bridge/settings.json` (device-level UI preferences)
- **Backups**: `~/.ykw-bridge/backups/` (auto-rotated, keeps 10 most recent)
- **Skills**: `~/.ykw-bridge/skills/` (synced into Claude workflows by default)
- **Skill Backups**: `~/.ykw-bridge/skill-backups/` (created automatically before uninstall, keeps 20 most recent)

</details>

## Documentation

For detailed guides on every feature, check out the **[User Manual](docs/user-manual/en/README.md)** — covering provider management, MCP/Prompts/Skills, proxy & failover, and more.

## Quick Start

### Basic Usage

1. **Add Provider**: Click "Add Provider" → Choose a preset or create custom configuration
2. **Switch Provider**:
   - Main UI: Select provider → Click "Enable"
   - System Tray: Click provider name directly (instant effect)
3. **Takes Effect**: Restart your terminal or the corresponding CLI tool to apply changes (Claude Code does not require a restart)
4. **Back to Official**: Add an "Official Login" preset, restart the CLI tool, then follow its login/OAuth flow

### MCP, Prompts, Skills & Sessions

- **MCP**: Click the "MCP" button → Add servers via templates or custom config → Sync to the Claude path
- **Prompts**: Click "Prompts" → Create presets with Markdown editor → Activate to sync to live files
- **Skills**: Click "Skills" → Browse GitHub repos → One-click install into Claude workflows
- **Sessions**: Click "Sessions" → Browse, search, and restore Claude conversation history

> **Note**: On first launch, you can manually import existing Claude configs as the default provider.

## Download & Installation

### System Requirements

- **Windows**: Windows 10 and above
- **macOS**: macOS 12 (Monterey) and above
- **Linux**: Ubuntu 22.04+ / Debian 11+ / Fedora 34+ and other mainstream distributions

### Windows Users

Download the latest Windows installer (`.msi`) or portable archive (`.zip`) from the [Releases](../../releases) page.

### macOS Users

**Manual Download (Recommended for now)**

Download the latest macOS `.dmg` (recommended) or `.zip` from the [Releases](../../releases) page.

> **Note**: YKW Bridge for macOS is code-signed and notarized by Apple. You can install and open it directly.

### Arch Linux Users

For now, use the latest `.AppImage` from the [Releases](../../releases) page.

### Linux Users

Download the latest Linux build from the [Releases](../../releases) page:

- `.deb` (Debian/Ubuntu)
- `.rpm` (Fedora/RHEL/openSUSE)
- `.AppImage` (Universal)

> **Flatpak**: Not included in official releases. You can build it yourself from the `.deb` — see [`flatpak/README.md`](flatpak/README.md) for instructions.

<details>
<summary><strong>Architecture Overview</strong></summary>

### Design Principles

```
┌─────────────────────────────────────────────────────────────┐
│                    Frontend (React + TS)                    │
│  ┌─────────────┐  ┌──────────────┐  ┌──────────────────┐    │
│  │ Components  │  │    Hooks     │  │  TanStack Query  │    │
│  │   (UI)      │──│ (Bus. Logic) │──│   (Cache/Sync)   │    │
│  └─────────────┘  └──────────────┘  └──────────────────┘    │
└────────────────────────┬────────────────────────────────────┘
                         │ Tauri IPC
┌────────────────────────▼────────────────────────────────────┐
│                  Backend (Tauri + Rust)                     │
│  ┌─────────────┐  ┌──────────────┐  ┌──────────────────┐    │
│  │  Commands   │  │   Services   │  │  Models/Config   │    │
│  │ (API Layer) │──│ (Bus. Layer) │──│     (Data)       │    │
│  └─────────────┘  └──────────────┘  └──────────────────┘    │
└─────────────────────────────────────────────────────────────┘
```

**Core Design Patterns**

- **SSOT** (Single Source of Truth): All data stored in `~/.ykw-bridge/ykw-bridge.db` (SQLite)
- **Dual-layer Storage**: SQLite for syncable data, JSON for device-level settings
- **Dual-way Sync**: Write to live files on switch, backfill from live when editing active provider
- **Atomic Writes**: Temp file + rename pattern prevents config corruption
- **Concurrency Safe**: Mutex-protected database connection avoids race conditions
- **Layered Architecture**: Clear separation (Commands → Services → DAO → Database)

**Key Components**

- **ProviderService**: Provider CRUD, switching, backfill, sorting
- **McpService**: MCP server management, import/export, live file sync
- **ProxyService**: Local proxy mode with hot-switching and format conversion
- **SessionManager**: Conversation history browsing across all supported apps
- **ConfigService**: Config import/export, backup rotation
- **SpeedtestService**: API endpoint latency measurement

</details>

<details>
<summary><strong>Development Guide</strong></summary>

### Environment Requirements

- Node.js 18+
- pnpm 8+
- Rust 1.85+
- Tauri CLI 2.8+

### Development Commands

```bash
# Install dependencies
pnpm install

# Dev mode (hot reload)
pnpm dev

# Type check
pnpm typecheck

# Format code
pnpm format

# Check code format
pnpm format:check

# Run frontend unit tests
pnpm test:unit

# Run tests in watch mode (recommended for development)
pnpm test:unit:watch

# Build application
pnpm build

# Build debug version
pnpm tauri build --debug
```

### Rust Backend Development

```bash
cd src-tauri

# Format Rust code
cargo fmt

# Run clippy checks
cargo clippy

# Run backend tests
cargo test

# Run specific tests
cargo test test_name

# Run tests with test-hooks feature
cargo test --features test-hooks
```

### Testing Guide

**Frontend Testing**:

- Uses **vitest** as test framework
- Uses **MSW (Mock Service Worker)** to mock Tauri API calls
- Uses **@testing-library/react** for component testing

**Running Tests**:

```bash
# Run all tests
pnpm test:unit

# Watch mode (auto re-run)
pnpm test:unit:watch

# With coverage report
pnpm test:unit --coverage
```

### Tech Stack

**Frontend**: React 18 · TypeScript · Vite · TailwindCSS 3.4 · TanStack Query v5 · react-i18next · react-hook-form · zod · shadcn/ui · @dnd-kit

**Backend**: Tauri 2.8 · Rust · serde · tokio · thiserror · tauri-plugin-updater/process/dialog/store/log

**Testing**: vitest · MSW · @testing-library/react

</details>

<details>
<summary><strong>Project Structure</strong></summary>

```
├── src/                        # Frontend (React + TypeScript)
│   ├── components/
│   │   ├── providers/          # Provider management
│   │   ├── mcp/                # MCP panel
│   │   ├── prompts/            # Prompts management
│   │   ├── skills/             # Skills management
│   │   ├── sessions/           # Session Manager
│   │   ├── proxy/              # Proxy mode panel
│   │   ├── settings/           # Settings (Terminal/Backup/About)
│   │   ├── deeplink/           # Deep Link import
│   │   ├── env/                # Environment variable management
│   │   ├── usage/              # Usage statistics
│   │   └── ui/                 # shadcn/ui component library
│   ├── hooks/                  # Custom hooks (business logic)
│   ├── lib/
│   │   ├── api/                # Tauri API wrapper (type-safe)
│   │   └── query/              # TanStack Query config
│   ├── locales/                # Translations (zh/en/ja)
│   ├── config/                 # Presets (providers/mcp)
│   └── types/                  # TypeScript definitions
├── src-tauri/                  # Backend (Rust)
│   └── src/
│       ├── commands/           # Tauri command layer (by domain)
│       ├── services/           # Business logic layer
│       ├── database/           # SQLite DAO layer
│       ├── proxy/              # Proxy module
│       ├── session_manager/    # Session management
│       ├── deeplink/           # Deep Link handling
│       └── mcp/                # MCP sync module
├── tests/                      # Frontend tests
└── assets/                     # Screenshots & partner resources
```

</details>

## Contributing

Issues and suggestions are welcome!

Before submitting PRs, please ensure:

- Pass type check: `pnpm typecheck`
- Pass format check: `pnpm format:check`
- Pass unit tests: `pnpm test:unit`

For new features, please open an issue for discussion before submitting a PR. PRs for features that are not a good fit for the project may be closed.

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=SherlockShemol/ykw-bridge&type=Date)](https://www.star-history.com/#SherlockShemol/ykw-bridge&Date)

## License

Licensed under the MIT License. See [LICENSE](LICENSE) for details. The upstream copyright notice is retained in accordance with the license.
