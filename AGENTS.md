# AGENTS.md

Workflow: Check Skills -> Expand Relevant Skill -> Execute Plan -> Answer User.

Despite the crate name, this is **not** a LeetCode solution repo. It is a single-binary
egui desktop app: a macOS-style "Dynamic Island" for a temporary file shelf, music, a focus
timer, and notifications, built on top of the Windows system media session (SMTC).

## Commands

```powershell
cargo run                # launch the app (Windows target)
cargo check              # fast feedback; the only required gate
cargo test               # 9 tests (storage + animation + notifications); fast, no external deps
cargo test shelf         # just the storage/shelf tests
cargo test animation     # just the page-transition tests
cargo clippy --all-targets
cargo build --release    # what users actually get: console window disappears
```

- Toolchain: `stable-x86_64-pc-windows-msvc`, edition 2024 (needs a recent rustc).
- No rustfmt/clippy config, no CI, no README, no pre-commit hooks. Don't invent them.
- `cargo test` and `cargo clippy` are not required by any config; `check` is the practical gate.
- Every source file is a module of `main.rs`. There are no lib targets and no workspace.

## Non-obvious structure

- `src/main.rs` only sets up the window and 60/20 fps frame pacing; real logic is `src/app/mod.rs`.
- `src/app/mod.rs` owns all state (`DynamicIsland`) plus **Windows-only** code: `strip_native_caption`
  and `update_native_region` (Win32 `SetWindowRgn`). Everything else is platform-neutral.
- `src/app/panels.rs` renders the four tabs. Tabs are indices, not an enum; use the constants
  `SHELF_TAB` / `SHORTCUTS_TAB` / `MUSIC_TAB` / `TIMER_TAB` / `ALERTS_TAB` from `app/mod.rs`
  rather than literals. Adding a tab means updating `TABS`, those constants, the `match` in
  `page()`, the header title in `expanded_view`, and the shortcut handler.
- **All tab changes must go through `DynamicIsland::set_tab` / `step_tab`.** They start the
  page transition; assigning `self.tab` directly silently breaks the animation and leaves the
  transition pointing at the wrong page. `src/animation.rs` holds both `Spring` (island morph)
  and `PageTransition` (directional page slide).
- `src/storage.rs` is the temporary file shelf: paths only, never copies. It also holds the
  Windows drag-**out** implementation (`DoDragDrop` + `CF_HDROP`), which has no egui/eframe
  equivalent — eframe only handles drags coming *in*.
- `src/media.rs` has two `platform` modules gated by `#[cfg(windows)]` / `#[cfg(not(windows))]`.
  The non-Windows one is a stub. If you add a method to the Windows impl, **you must add the
  matching stub** or non-Windows builds break. Same pattern for `SystemMedia` itself, and for
  `storage::drag_out` (`false` on non-Windows).
- `src/volume.rs` owns output volume. `SystemVolume` is the Windows default-endpoint
  `IAudioEndpointVolume` (Core Audio) and has a non-Windows stub; `VolumeState` caches the level
  plus the mute bookkeeping for the music page slider. The slider is always the **system**
  volume — in both music views, because a local track plays through that same device. It never
  adds a second gain: the island's own decoder stays at a fixed `audio::VOLUME`.
- `src/shortcuts.rs` is the saved-command list: a name, a command line, and an optional working
  directory, run through `cmd /C` with `CREATE_NO_WINDOW` and persisted as JSON under
  `%APPDATA%\leetcode\shortcuts.json` (`LEETCODE_CONFIG_DIR` overrides it; the tests rely on
  that so they never touch the real config). It is not a config format: don't hand-edit it from
  the UI code.
- Several commands may run at once, tracked in `Shortcuts::running` and keyed by name, so a
  second click on the same tile is refused rather than starting a duplicate dev server. `tick()`
  reaps finished children for their exit code; `kill(name)` and `kill_all()` stop a command
  **and its descendants**. That tree walk matters: `cmd /C npm run dev` exits as soon as it
  hands off, but `node` keeps the port bound, so the child pids are snapshotted at spawn and
  re-read through `tasklist` at kill time, then killed with `taskkill /T /F`. Killing only the
  wrapper would orphan the server.
- `src/icons.rs` supplies the real shell icon for a shelf file or a shortcut target
  (`SHGetFileInfoW` → `HICON` → 32-bit DIB via GDI → RGBA). Non-Windows returns `None`. Icons are
  decoded once per path and cached as textures in `DynamicIsland::icons` — never call
  `for_path` from a paint loop, go through `DynamicIsland::icon_for`. Callers must handle `None`
  by drawing the monogram tile (`ui::file_tile`), which is also what an icon-less extension gets.
- `src/ui.rs` is the drawing library: colors, sizes (`IDLE_SIZE`, `EXPANDED_SIZE`), hand-drawn
  icons (`Icon`, `paint_icon`, `nav_button`), grid tiles (`grid_columns`, `grid_cell`,
  `file_tile`) and layout primitives. Prefer these over `egui` defaults.

## Gotchas that will bite you

- **Page transitions draw two live pages.** While a transition runs (~260 ms) both the outgoing
  and incoming pages are rendered into a fixed-size, clipped area. The outgoing page is
  `disable()`d so a click cannot hit a control that is already sliding away; don't remove that.
  Do not use `available_height()` on the outer UI expecting it to shrink — the content area is
  pinned so the layout cannot reflow mid-slide.
- **New UI animations need frame pacing.** Anything time-based must be added to `busy` in
  `request_repaint`, or the animation will only advance at the 100 ms idle rate.
- **Music tab prefers the OS.** If `system_media` is `Some`, `music_tab` renders the system session
  and completely ignores the app's own `Audio` decoder. `Space` follows the same rule. Don't add
  local-playback UI without accounting for this branch.
- **Never use Unicode glyphs for icons.** Default fonts lack them; `ui::icon_button` and
  `ui::notification_glyph` map glyph/kind strings to vector paths. To add an icon, add a path in
  `paint_icon` (`Icon` enum) rather than a new character.
- **Never use `ui.label` for text that can be long.** Track titles and filenames overflow the
  island. Use `ui::truncate` / `ui::fit_to`, which clip to one line with an ellipsis.
- **Right-aligned widgets need `right_area` / a right-to-left child `Ui`.** `horizontal()` and
  `with_layout(RIGHT_TO_LEFT)` lay out from the *left* edge of the remaining space, so a naive
  right-aligned button lands past the island padding. The existing helpers in `panels.rs` exist
  precisely for this; reuse them.
- **Win32 window code is re-applied every frame on purpose.** winit rewrites the window style on
  any flag change and the native caption flashes back. `strip_native_caption` re-checks and clears
  frame bits each frame; `update_native_region` caches the applied region in `applied_region` and
  calls `SetWindowRgn` only when the geometry actually changes (calling it every frame flashes a
  native frame). Do not "optimize" either by moving it out of the per-frame path.
- **The transparent window needs a `CentralPanel`.** `ui()` wraps its content in one so
  `clear_color` is applied; without it the window renders white.
- **Drag-out blocks the UI thread.** `storage::drag_out` calls `DoDragDrop`, which does not
  return until the user drops or presses Escape. This is inherent to Win32 drag-and-drop; don't
  "fix" it by moving it to a thread (the OLE/COM apartment belongs to the UI thread). It is only
  invoked from `drag_stopped()`, i.e. after the user has already released the button.
- **`windows-core` must stay a direct dependency.** The `#[implement(IDataObject)]` /
  `#[implement(IDropSource)]` macros expand to code referencing the `windows_core` crate by name.
  Removing it from `Cargo.toml` breaks the build even though nothing imports it directly.
- **Drag-out needs specific `windows` features** beyond what the media code uses:
  `Win32_System_Ole`, `Win32_System_SystemServices` (for `MODIFIERKEYS_FLAGS`/`MK_LBUTTON`),
  `Win32_UI_Shell` (for `DROPFILES`), `Win32_System_Memory`, and
  `Win32_System_Com_StructuredStorage`. Note `GlobalFree` lives in `Win32_Foundation`, not
  `Win32_System_Memory`, and the shell helpers have no `Win32_System_Shell` module.
- **System media polling is a blocking WinRT call.** `poll_system_media` throttles to every 700 ms
  and is best-effort: `SystemMedia::new()` returns `None` on older Windows, and every WinRT call
  uses `ok()?` / `is_some_and` so the app stays usable. Between polls, playback position is
  extrapolated locally in `media_position`; after any seek/toggle the poll timer is reset by
  rewinding `last_media_poll` — keep that when adding new media actions.
- Seeks to the OS are **relative** (`seek_by` takes 100 ns ticks); an absolute click position on
  the progress bar is converted to a delta first (see `system_music_panel`).
- Audio: `rodio` device sink is stored as `_sink` in `Engine` — dropping it stops playback.
  Looping is manual via `Audio::tick` re-loading the file when the mixer runs dry. There is no
  per-track volume: output level belongs to the system device (see `src/volume.rs`).
- System volume polling is another blocking Core Audio call. `poll_system_volume` throttles to
  every 700 ms and must not stomp a value the user just dragged in: any volume action rewinds
  `last_volume_poll` (`volume_changed`). `SystemVolume::new` only calls `CoUninitialize` when it
  was the call that initialised COM, since the endpoint object must outlive the factory call.
- Debug vs release: release uses `windows_subsystem = "windows"` (no console); debug keeps the
  console but hides it in `hide_console()`. Both matter for whether `cargo run` shows a terminal.

## Conventions

- `Cargo.toml` uses `opt-level = 1` for this crate and `3` for all dependencies in dev — this is
  why `cargo run` builds are already usable. Don't lower dependency opt-level.
- UI constants live in `ui.rs`, not inline in call sites; spacing values are usually bare `f32`
  literals passed to `ui.add_space`.
- Verify visual/Windows behavior by running the app; there is no screenshot or UI test harness.
