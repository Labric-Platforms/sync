# Labric Sync

Desktop file sync client built with Tauri 2, React 19, and Rust. Monitors a local folder and automatically uploads changes to the [Labric](https://labric.co) platform via presigned URLs.

## Features

- **Real-time file watching** -- Native OS file watcher (via `notify` crate) detects created, modified, and deleted files
- **Batched uploads** -- Queues changes with configurable delay, uploads concurrently with retry logic and CRC32C integrity checks
- **Device heartbeat** -- Reports device status and app version to the server every 30 seconds
- **Pattern-based ignoring** -- Glob patterns to exclude files from sync (e.g. `*.tmp`, `.git/**`, `node_modules/**`)
- **Auto-updates** -- Signed updates delivered via GitHub Releases and the Tauri updater plugin
- **Code-signed builds** -- macOS (Apple notarized) and Windows (DigiCert KeyLocker) signing in CI

## Tech Stack

| Layer    | Technology |
|----------|-----------|
| Frontend | React 19, TypeScript, TanStack Router, Tailwind CSS v4, shadcn/ui |
| Backend  | Rust, Tauri 2, Tokio, notify, reqwest |
| Build    | Vite 8, pnpm |
| CI/CD    | GitHub Actions (multi-platform matrix build) |

## Architecture

```
Frontend (React)          Backend (Rust/Tauri)          Labric Platform
─────────────────    IPC   ──────────────────────   HTTP   ──────────────
Login UI          ←─────→  Device info commands   ──────→  /api/sync/*
Dashboard         ←─────→  File watcher (notify)  ──────→  Presigned uploads
Upload progress   ←events─ Upload queue processor
Settings          ←─────→  Heartbeat service      ──────→  /api/sync/heartbeat
```

**Frontend** communicates with **Rust** via Tauri IPC commands and events. The Rust backend handles file watching, upload queue processing, and heartbeat reporting over HTTP to the Labric platform API.

## Development

### Prerequisites

- [Node.js](https://nodejs.org/) (LTS)
- [pnpm](https://pnpm.io/)
- [Rust](https://www.rust-lang.org/tools/install)
- [Tauri v2 prerequisites](https://v2.tauri.app/start/prerequisites/)

### Setup

```bash
pnpm install
pnpm tauri dev
```

The Vite dev server runs on port 1420. Tauri opens a native window pointing to it with hot reload enabled.

### Build

```bash
pnpm tauri build
```

Produces platform-specific installers (`.dmg`, `.msi`, `.AppImage`).

### Generate API Types

```bash
pnpm types
```

Generates TypeScript types from the backend OpenAPI spec at `http://localhost:8000/api/openapi.json`.

## Project Structure

```
src/                        # React frontend
  routes/                   # File-based routing (TanStack Router)
    __root.tsx              # Root layout, theme, auth check
    index.tsx               # Redirects to dashboard
    login.tsx               # Device pairing flow
    _protected/
      route.tsx             # Auth guard layout
      dashboard.tsx         # Main file watcher dashboard
  components/               # UI components (shadcn/ui + custom)
  hooks/                    # useUploadManager, useHeartbeat, useAppUpdater, use-session-context
  lib/                      # Tauri store wrapper, utils
  types/                    # TypeScript type definitions

src-tauri/                  # Rust backend
  src/
    lib.rs                  # App setup, Tauri commands, file watcher
    upload.rs               # Upload queue processor
    heartbeat.rs            # Heartbeat service
    diagnostics.rs          # Network diagnostics (connectivity checks)
    http_client.rs          # Shared HTTP client + response helpers
  tauri.conf.json           # Tauri app configuration
  Cargo.toml                # Rust dependencies
```

## Authentication Flow

1. App generates a device fingerprint and requests a 6-digit pairing code from the server
2. The code is displayed on the login screen -- user visits `labric.co/pair` and enters it to link the device
3. App polls `/api/sync/poll-pairing` until the code is confirmed, then receives a JWT token
4. Token is persisted in Tauri Store for subsequent sessions

## CI/CD

Pushing to `main` triggers a GitHub Actions workflow that builds for macOS (x86_64 + aarch64), Linux, and Windows. Builds are code-signed and published as draft GitHub Releases with updater artifacts.

## Environment Variables

| Variable | Description |
|----------|-------------|
| `VITE_SERVER_URL` | Backend API base URL (default: `http://localhost:3000`) |

## License

Proprietary -- Labric Platforms
