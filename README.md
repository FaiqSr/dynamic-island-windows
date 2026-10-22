<div align="center">

# Dynamic Island

**A compact Windows desktop companion for files, music, focus sessions, and notifications.**

[![Rust](https://img.shields.io/badge/Rust-2024-000000?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![egui](https://img.shields.io/badge/UI-egui-7D5FFF)](https://github.com/emilk/egui)
[![Platform](https://img.shields.io/badge/platform-Windows-0078D4?logo=windows&logoColor=white)](https://www.microsoft.com/windows)

</div>

---

## Overview

Dynamic Island is a lightweight, always-on-top desktop island. Hover near the
top-center of the screen to reveal it, then click to expand its pages. Swipe
horizontally through pages; navigation loops in both directions.

## Features

- **Temporary shelf** — collect files and drag them into another application.
- **Shortcuts** — save and launch commands; stop running commands and their
  child processes from the app.
- **Music** — control the active Windows media session or play a local audio
  file. Seek, adjust system volume, and mute from one page.
- **Focus timer** — choose a preset or custom duration, pause, reset, or extend
  a session.
- **Alerts** — review and dismiss in-app notifications.
- **Settings** — optionally launch at Windows sign-in or close the app.
- **Compact controls** — browse pages with horizontal swipes or the page
  navigation buttons.

## Requirements

- Windows 10 or later
- Rust stable with the `x86_64-pc-windows-msvc` target
- Visual Studio C++ Build Tools (MSVC)

## Run locally

```powershell
git clone <repository-url>
cd <repository-directory>
cargo run
```

Build the release executable:

```powershell
cargo build --release
```

The executable is written to `target\release\leetcode.exe`.

## Checks

```powershell
cargo check
cargo test
```

## Startup behavior

Use **Settings → Launch at sign-in** to add or remove the current executable
from the current Windows user's startup registry key. Keep the executable in a
stable location for startup to continue working after sign-in.

## Project layout

| Path | Purpose |
| --- | --- |
| `src/main.rs` | Window setup and frame pacing |
| `src/app/` | Application state, page navigation, and UI panels |
| `src/ui.rs` | Shared drawing primitives, colors, and island sizing |
| `src/media.rs` | Windows system media session integration |
| `src/audio.rs` | Local audio playback |
| `src/volume.rs` | System output volume control |
| `src/storage.rs` | Temporary file shelf and Windows drag-out |
| `src/shortcuts.rs` | Saved commands and child-process management |
| `src/timer.rs` | Focus timer state and countdown |

## Notes

- Shelf entries store paths; files are not copied into the application.
- Music volume controls the Windows output device, including local playback.
- The startup option is per-user and does not require administrator access.
