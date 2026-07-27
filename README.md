# Codex Pacer

A system tray app that tells you whether to slow down — not just how much of your [Codex CLI](https://github.com/openai/codex) usage is left.

Most usage trackers show raw numbers: session %, weekly %, credits. Codex Pacer compares your actual usage against a straight-line target trajectory to the next reset and gives you a plain-language verdict: **Slow down**, **On track**, or **Room to use more**. Built primarily for **Windows**, with **Linux** as a secondary target (same codebase, via Tauri).

Inspired by [codex-limits](https://github.com/thrr87/codex-limits) (macOS/SwiftUI), rewritten to run outside the Apple ecosystem.

> Codex Pacer is an independent, unofficial project. Not affiliated with or endorsed by OpenAI.

## Why this instead of a usage dashboard?

There are excellent multi-provider dashboards out there (e.g. [CodexBar](https://github.com/steipete/CodexBar)) if you want every AI tool's usage in one place. Codex Pacer does one thing instead: it tells you, right now, whether your pace of usage will get you to the reset without running out — or whether you're being too conservative and could use more.

It's also deliberately narrow in scope: only Codex, no browser cookies, no OAuth, no Keychain/credential-store access. It talks to the `codex` CLI you already have installed and logged in, the same way the official Codex clients do.

## What it does

- Reads real usage from the Codex CLI's `app-server` JSON-RPC interface (see [`docs/app-server-protocol.md`](docs/app-server-protocol.md)).
- Compares actual usage against a target trajectory (100% at window start, down to a safety buffer at reset) and shows a burn-down chart plus the verdict.
- Records local usage samples (daily JSON files, 90-day retention) to track actual pace over time.
- Auto-refreshes on window focus, every 10 minutes, and on demand.
- Runs as a native tray app with no third-party runtime dependencies bundled into the app itself.

## Privacy

- No credentials are read, stored, or transmitted by Codex Pacer. It starts the user-managed `codex` CLI and talks to its local `app-server` interface — the same credentials the CLI itself already has.
- Usage samples are stored locally as JSON files under the OS app-data directory (`%LOCALAPPDATA%\com.bielcx.codexpacer` on Windows, `~/.local/share/com.bielcx.codexpacer` on Linux).
- No telemetry, no analytics, no network calls other than what the Codex CLI itself makes.

## Install

Prebuilt Windows and Linux builds are attached to [GitHub Releases](../../releases) (installer `.exe`/`.msi` for Windows, `.deb`/`.AppImage` for Linux).

Requires the [Codex CLI](https://github.com/openai/codex) installed and logged in (`npm i -g @openai/codex`, then `codex login`).

## Development

### Requirements

- [Rust](https://www.rust-lang.org/tools/install) (stable toolchain) + Cargo
- [Node.js](https://nodejs.org/) 18+
- Windows: [Build Tools for Visual Studio](https://tauri.app/start/prerequisites/) (C++ workload) and WebView2 (bundled with Windows 11; may need installing on Windows 10)
- Linux: system dependencies listed in [Tauri's prerequisites](https://tauri.app/start/prerequisites/#linux) (webkit2gtk, libappindicator, etc.)

### Running in dev mode

```bash
npm install
npm run dev
```

### Tests

```bash
cd src-tauri
cargo test
```

### Build

```bash
npm run build
```

Produces Windows installers (NSIS/MSI) or, when run on Linux, `.deb`/AppImage packages, in `src-tauri/target/release/bundle/`.

### Regenerating icons

Icons are committed under `src-tauri/icons/`. To regenerate them from a new source image (square PNG, ideally 1024x1024):

```bash
npx tauri icon path/to/logo.png
```

## Roadmap

- [x] Read real usage from the Codex CLI app-server
- [x] Local usage history
- [x] Pacing verdict + burn-down chart
- [x] Popover UI + auto-refresh
- [x] Packaging + release pipeline
- [ ] Token/cost tracking (via `account/usage/read`)
- [ ] Configurable safety buffer in a settings UI

## License

MIT.
