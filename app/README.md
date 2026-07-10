# Echoless — Desktop GUI (Tauri)

English | [简体中文](README.zh-CN.md)

Tauri v2 + React/TS implementation of the Echoless control-appliance main UI. The
source of truth for the finalized visuals/interaction is
`AEC/Design/overview.html` + `AEC/Design/Design.md`.

## Architecture

```
React/TS UI  ──invoke──▶  Tauri (Rust, src-tauri/src/*)
   ▲                          │ spawn sidecar
   └── echoless://status ◀────┴── echoless CLI (--status-json JSONL)
```

The Rust side is split into modules by responsibility (`src-tauri/src/`): `lib.rs`
is entry point + setup only; `sidecar.rs` (run lifecycle / hot commands),
`bin_resolve.rs` (binary resolution), `proc.rs`, `localvqe.rs`, `nvafx.rs`,
`platform.rs`, `device_watch.rs`, `tray.rs`, `commands.rs` (thin
`#[tauri::command]` wrappers).

- Consumes only the JSON / JSONL contract (`types.ts` mirrors the backend shapes); it does not parse human-readable logs (those go through stderr → `echoless://log`).
- One-shot commands: `list_devices` / `list_processors` / `validate_config`.
- Real-time: `start_run` (`sidecar.rs`) spawns `echoless run --status-json --stats-interval-ms 80`, parses it line by line, and pushes it to the frontend via `echoless://status` events; `stop_run` closes stdin → waits with a timeout → kills (graceful shutdown); on exit (window close / Cmd+Q / ExitRequested) the child process is reaped automatically.

## Resolving the echoless binary

`src-tauri/src/bin_resolve.rs::echoless_bin()`:

1. Developer override from the `ECHOLESS_BIN` environment variable.
2. `echoless` or `echoless-<target-triple>` next to the current executable (the packaged `externalBin` location).
3. `echoless`, `binaries/echoless`, or the target-triple binary in the Tauri resource directory.
4. Dev target-triple binary under `src-tauri/binaries/`.
5. Repo-root `target/release/echoless`, then `target/debug/echoless`.

In dev, build the CLI at the repo root first:

```bash
cd ..            # echoless/
cargo build --release -p echoless-cli
```

## Running it

```bash
pnpm install
pnpm tauri dev          # development (hot-reloads the frontend + Rust)
pnpm tauri build        # packaging
```

## Platform title bar (Design.md §5.1)

The window is created programmatically (`lib.rs` setup), mirroring the platform:

- **macOS**: `TitleBarStyle::Overlay` + `hidden_title`, keeping the system traffic lights (drawn by the OS, top-left); `set_traffic_lights_inset(16,13)` centers the traffic lights within the 40px title bar.
- **Windows / Linux**: `decorations(false)` + `shadow(true)`, with self-drawn caption buttons (top-right `─ □ ✕`, close turns red on hover).

The platform is returned by the `get_platform` command, and the frontend switches between `.window.mac` / `.window.win`.

The window is created with `visible(false)`; once the frontend's first screen is
ready (`booted`, with fonts + the first batch of data in place, hard-capped at
1.2s) it is shown via the core window show permission — eliminating the white
flash during WebView initialization. The Rust side also has a 5s fallback to
prevent the window from never appearing if the frontend crashes.

## Current boundaries

- **Real waveforms**: the backend already emits `mic_wave/ref_wave/out_wave` (64-bucket peak envelope) in the status event, which `Scope.tsx` draws directly; it only falls back to a synthetic envelope when the waveform fields are absent.
- **sidecar packaging**: `tauri.conf.json` already declares `externalBin` and bundle resources (including the third-party licenses under `licenses/`); before packaging locally you still need to build the release CLI first and run `pnpm prepare:tauri-assets`.
- **Virtual sound card onboarding**: Mic Setup already integrates `doctor audio --json` detection and platform hints; driver installation is still done by the user.
- **Advanced / Diagnostics**: the pages are usable; when adding new diagnostic fields, update `types.ts` and the page display in sync.
- **LocalVQE / RTX**: the LocalVQE model download/selection and the RTX runtime install wizard are integrated, but they still depend on the corresponding platform-native assets being available.
