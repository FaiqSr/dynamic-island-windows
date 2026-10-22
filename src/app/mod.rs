use eframe::egui::{self, Align, Layout, Rect, Sense, Vec2};

use crate::ACTIVE_FRAME;
use crate::animation::{PageTransition, Spring};
use crate::audio::Audio;
use crate::media::{Snapshot, SystemMedia};
use crate::notifications::{NotificationView, Notifications};
use crate::shortcuts::Shortcuts;
use crate::storage::Shelf;
use crate::timer::{Timer, TimerState};
use crate::ui;
use crate::volume::{SystemVolume, VolumeState};
use std::time::Instant;

mod panels;

pub const TABS: [&str; 6] = ["Shelf", "Shortcuts", "Music", "Timer", "Alerts", "Settings"];

/// Tab indices. The shelf sits first so it is the default landing view.
pub const SHELF_TAB: usize = 0;
pub const SHORTCUTS_TAB: usize = 1;
pub const MUSIC_TAB: usize = 2;
pub const TIMER_TAB: usize = 3;
pub const ALERTS_TAB: usize = 4;
pub const SETTINGS_TAB: usize = 5;

pub struct DynamicIsland {
    pub expanded: bool,
    pub spring: Spring,
    pub tab: usize,
    /// Direction-aware slide/fade between pages; only active while a transition runs.
    pub transition: PageTransition,
    pub audio: Audio,
    pub timer: Timer,
    pub notifications: Notifications,
    /// Files parked temporarily so they can be dragged into another app.
    pub shelf: Shelf,
    /// Saved shell commands (`npm run dev`, `code .`, ...), persisted to a JSON config.
    pub shortcuts: Shortcuts,
    /// Draft of the shortcut being added: name, command, and working directory.
    pub shortcut_name: String,
    pub shortcut_command: String,
    pub shortcut_dir: String,
    /// The add form takes over the whole page, so the grid has to make way for it.
    pub shortcut_form_open: bool,
    /// Shell icons for the shelf and the shortcut grid, keyed by the path they were read from.
    /// Uploading a texture is not free, so each path is decoded and uploaded once.
    pub icons: std::collections::HashMap<String, Option<egui::TextureHandle>>,
    pub custom_minutes: u64,
    /// System media session (Spotify, browsers, ...) mirrored from Windows.
    pub system: Option<SystemMedia>,
    pub system_media: Option<Snapshot>,
    /// Output volume of the default playback device; `None` when Core Audio is unavailable.
    pub system_volume: Option<SystemVolume>,
    /// Level and mute state of the system output. `SystemVolume` is the actual writer; this is
    /// the cached copy the slider reads, refreshed from the device by `poll_system_volume`.
    pub volume: VolumeState,
    last_volume_poll: Instant,
    last_media_poll: Instant,
    hover_close_at: Option<Instant>,
    page_swipe_start: Option<egui::Pos2>,
    /// The window region currently applied, so it is only re-applied when it changes.
    /// Re-applying it every frame invalidated the window and could flash a native frame.
    #[cfg(windows)]
    applied_region: std::cell::RefCell<Option<[[i32; 4]; 3]>>,
    #[cfg(windows)]
    hwnd: Option<isize>,
}

impl DynamicIsland {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        #[cfg(windows)]
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        #[cfg(windows)]
        let hwnd = cc
            .window_handle()
            .ok()
            .and_then(|handle| match handle.as_raw() {
                RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
                _ => None,
            });

        cc.egui_ctx.set_visuals(egui::Visuals {
            dark_mode: true,
            window_fill: egui::Color32::TRANSPARENT,
            panel_fill: egui::Color32::TRANSPARENT,
            ..egui::Visuals::dark()
        });
        let system_volume = SystemVolume::new();
        // Start from the real device level so the slider is correct on the very first frame,
        // even though the first poll is still 700 ms away.
        let mut volume = VolumeState::new();
        if let Some(system) = system_volume.as_ref() {
            volume.sync(Some(system.level()));
        }
        Self {
            expanded: false,
            spring: Spring::new(0.0),
            tab: 0,
            transition: PageTransition::new(0),
            audio: Audio::new(),
            timer: Timer::new(),
            notifications: Notifications::new(),
            shelf: Shelf::new(),
            shortcuts: Shortcuts::load(),
            shortcut_name: String::new(),
            shortcut_command: String::new(),
            shortcut_dir: String::new(),
            shortcut_form_open: false,
            icons: std::collections::HashMap::new(),
            custom_minutes: 5,
            system: SystemMedia::new(),
            system_media: None,
            system_volume,
            volume,
            last_volume_poll: Instant::now(),
            last_media_poll: Instant::now() - std::time::Duration::from_secs(1),
            hover_close_at: None,
            page_swipe_start: None,
            #[cfg(windows)]
            applied_region: std::cell::RefCell::new(None),
            #[cfg(windows)]
            hwnd,
        }
    }

    pub fn open(&mut self) {
        self.expanded = true;
    }

    pub fn close(&mut self) {
        self.expanded = false;
        self.hover_close_at = None;
    }

    /// Switches to `tab` with a directional slide. All tab changes must go through here so
    /// the transition never gets out of sync with the visible page.
    pub fn set_tab(&mut self, tab: usize) {
        let tab = tab.min(TABS.len() - 1);
        if tab == self.tab {
            return;
        }
        self.transition.start(self.tab, tab);
        self.tab = tab;
    }

    /// Advances by `delta` pages, wrapping around.
    pub fn step_tab(&mut self, delta: isize) {
        let len = TABS.len() as isize;
        let next = (self.tab as isize + delta).rem_euclid(len) as usize;
        self.set_tab(next);
    }

    fn request_repaint(&self, ctx: &egui::Context) {
        let busy = self.expanded
            || self.hover_close_at.is_some()
            || !self.spring.is_settled(0.0)
            || self.audio.is_playing()
            || self.system_media.as_ref().is_some_and(|m| m.playing)
            || !self.notifications.is_empty()
            || self.shortcuts.any_running()
            || self.timer.state() == TimerState::Running;
        // A running page transition needs full-rate frames even if nothing else is busy.
        let wait = if busy || self.transition.is_running(Instant::now()) {
            ACTIVE_FRAME
        } else {
            std::time::Duration::from_millis(100)
        };
        ctx.request_repaint_after(wait);
    }

    fn tick_logic(&mut self) {
        self.audio.tick();
        self.poll_system_media();
        self.poll_system_volume();
        self.notifications.reap();
        self.shelf.prune();
        self.shortcuts.tick();
        if self.timer.tick() {
            self.notifications
                .push("Timer", format!("{} finished", self.timer.label), "timer");
            self.open();
            self.set_tab(TIMER_TAB);
        }
    }

    /// Reads the OS media session a few times per second; each read is a blocking WinRT call.
    fn poll_system_media(&mut self) {
        let poll_every = std::time::Duration::from_millis(700);
        if self.last_media_poll.elapsed() < poll_every {
            return;
        }
        self.last_media_poll = Instant::now();
        let Some(system) = self.system.as_ref() else {
            return;
        };
        let next = system.snapshot();
        let track_changed = match (&self.system_media, &next) {
            (Some(old), Some(new)) => {
                old.title != new.title || old.artist != new.artist || old.app != new.app
            }
            (Some(_), None) | (None, Some(_)) => true,
            (None, None) => false,
        };
        if let Some(new) = next.as_ref().filter(|new| track_changed && new.playing) {
            let mut body = if new.artist.trim().is_empty() {
                new.app.clone()
            } else {
                format!("{} \u{00b7} {}", new.artist, new.app)
            };
            if body.is_empty() {
                body = "System media".into();
            }
            self.notifications
                .push(new.title.clone(), body, ui::NotificationKind::Music);
        }
        self.system_media = next;
    }

    /// Reads the OS output volume a few times per second, so the slider follows the hardware
    /// keys and the Windows mixer. The read is cheap but not free, so it is throttled.
    fn poll_system_volume(&mut self) {
        if self.last_volume_poll.elapsed() < std::time::Duration::from_millis(700) {
            return;
        }
        self.last_volume_poll = Instant::now();
        if let Some(system) = self.system_volume.as_ref() {
            // The OS has no separate mute flag exposed here, so zero simply means muted.
            self.volume.sync(Some(system.level()));
        }
    }

    /// Drags on the volume slider that arrive in the same frame as the poll would otherwise be
    /// overwritten by it; rewinding the timer keeps the user's value.
    fn volume_changed(&mut self) {
        self.last_volume_poll = Instant::now();
    }

    /// System playback position, advanced locally between polls so the bar stays smooth.
    pub fn media_position(&self) -> std::time::Duration {
        let Some(media) = self.system_media.as_ref() else {
            return std::time::Duration::ZERO;
        };
        if media.playing {
            media.position + self.last_media_poll.elapsed()
        } else {
            media.position
        }
    }

    pub fn media_toggle(&mut self) {
        if let Some(system) = self.system.as_ref() {
            system.play_pause();
        }
        self.last_media_poll = Instant::now() - std::time::Duration::from_secs(1);
    }

    pub fn media_seek(&mut self, seconds: i64) {
        if let Some(system) = self.system.as_ref() {
            system.seek_by(seconds * 10_000_000);
        }
        self.last_media_poll = Instant::now() - std::time::Duration::from_secs(1);
    }

    /// Whether the music page has a system output device to drive. The slider controls the OS
    /// volume in *both* music views: a local track plays through that same device, so muting it
    /// while its own decoder kept its level would show a slider that contradicts what you hear.
    pub fn has_system_volume(&self) -> bool {
        self.system_volume.is_some()
    }

    /// Level currently shown by the music page slider: the OS device level. `VolumeState` only
    /// caches it, and is refreshed from the device by `poll_system_volume`.
    pub fn music_volume(&self) -> f32 {
        self.volume.level()
    }

    /// `None` when the slider has no mute button, i.e. the device volume could not be reached at
    /// all: muting then would be a dead control.
    pub fn music_muted(&self) -> Option<bool> {
        self.has_system_volume().then(|| self.volume.is_muted())
    }

    /// Writes `level` to the system output device.
    pub fn set_music_volume(&mut self, level: f32) {
        self.volume.set(level);
        if let Some(system) = self.system_volume.as_ref() {
            system.set_level(self.volume.level());
        }
        self.volume_changed();
    }

    /// Stops every command the Shortcuts page started. Exposed through the page's `Kill all`
    /// button, because a dev server keeps a port bound until it is killed explicitly.
    pub fn kill_all_shortcuts(&mut self) {
        self.shortcuts.kill_all();
    }

    /// Icon texture for `path`, decoded from the shell on first use and then reused.
    ///
    /// The cache is keyed by path *and* command, because the same executable can be the target
    /// of different shortcuts. A miss with no icon is cached as `None` too: a folder of missing
    /// files must not hit the shell every frame.
    pub fn icon_for(&mut self, ctx: &egui::Context, path: &std::path::Path, command: Option<&str>) -> Option<egui::TextureHandle> {
        let key = match command {
            Some(command) => format!("{}\u{1}{command}", path.display()),
            None => path.display().to_string(),
        };
        if let Some(cached) = self.icons.get(&key) {
            return cached.clone();
        }
        let texture = crate::icons::for_path(path, command).map(|pixels| {
            let image =
                egui::ColorImage::from_rgba_unmultiplied(pixels.dimensions(), &pixels.rgba);
            ctx.load_texture(
                format!("file-icon:{key}"),
                image,
                egui::TextureOptions::LINEAR,
            )
        });
        self.icons.insert(key, texture.clone());
        texture
    }

    /// Mutes the system output device, or restores the level from before the mute.
    pub fn toggle_music_mute(&mut self) {
        let level = self.volume.toggle_mute();
        if let Some(system) = self.system_volume.as_ref() {
            system.set_level(level);
        }
        self.volume_changed();
    }
}

impl eframe::App for DynamicIsland {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(windows)]
        self.strip_native_caption();
        self.tick_logic();
        self.take_dropped_files(ctx);
        self.request_repaint(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // A panel is required so the frame gets cleared with `clear_color`;
        // without it the transparent window renders white.
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let outer = ui::morph(
                    ui,
                    &mut self.spring,
                    self.expanded,
                    self.hover_close_at.is_some(),
                );
                let trigger = Rect::from_center_size(
                    egui::pos2(outer.center().x, outer.top() + 5.0),
                    egui::vec2(88.0, 10.0),
                );
                #[cfg(windows)]
                self.update_native_region(outer, trigger, ui.ctx().pixels_per_point());
                let trigger_hovered = ui
                    .interact(
                        trigger,
                        egui::Id::unique("island_hover_trigger"),
                        egui::Sense::hover(),
                    )
                    .hovered();
                let content_size = if self.expanded {
                    ui::EXPANDED_SIZE
                } else {
                    ui::IDLE_SIZE
                };
                let content_rect = Rect::from_center_size(
                    egui::pos2(outer.center().x, outer.top() + content_size.y * 0.5),
                    content_size,
                );
                let inner = content_rect.shrink(if self.expanded { ui::PAD } else { 0.0 });
                let mut content = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(inner)
                        .layout(Layout::top_down(Align::Min)),
                );
                content.set_clip_rect(outer);

                if self.expanded {
                    self.expanded_view(&mut content);
                } else {
                    self.idle_view(&mut content, outer);
                }

                self.handle_shortcuts(&content);
                self.handle_page_swipe(&ctx, content_rect);

                let island_hovered = ui.rect_contains_pointer(outer);
                let hovered = trigger_hovered || island_hovered;
                if !self.expanded {
                    if hovered {
                        self.hover_close_at =
                            Some(Instant::now() + std::time::Duration::from_millis(800));
                    } else if self
                        .hover_close_at
                        .is_some_and(|deadline| Instant::now() >= deadline)
                    {
                        self.hover_close_at = None;
                    }
                } else if hovered || ctx.input(|i| i.pointer.any_down()) {
                    self.hover_close_at = None;
                } else if let Some(deadline) = self.hover_close_at {
                    if Instant::now() >= deadline {
                        self.close();
                    }
                } else {
                    self.hover_close_at =
                        Some(Instant::now() + std::time::Duration::from_secs(2));
                }

                if !self.expanded {
                    self.floating_notification(ui, outer);
                    if ui
                        .interact(outer, egui::Id::unique("island_body"), egui::Sense::click())
                        .clicked()
                    {
                        self.open();
                    }
                    if ui.rect_contains_pointer(outer) {
                        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    self.close();
                }
            });

        self.request_repaint(&ctx);
    }

}

#[cfg(windows)]
impl DynamicIsland {
    fn handle_page_swipe(&mut self, ctx: &egui::Context, content_rect: Rect) {
        let (pressed, released, position) = ctx.input(|input| {
            (
                input.pointer.button_pressed(egui::PointerButton::Primary),
                input.pointer.button_released(egui::PointerButton::Primary),
                input.pointer.interact_pos(),
            )
        });

        if pressed {
            self.page_swipe_start = position.filter(|pos| {
                content_rect.contains(*pos)
                    && pos.y >= content_rect.top() + 92.0
                    && pos.y <= content_rect.bottom() - ui::PAD
            });
        }
        if released {
            if let (Some(start), Some(end)) = (self.page_swipe_start.take(), position) {
                let delta = end - start;
                if delta.x.abs() >= 64.0 && delta.y.abs() < 72.0 {
                    let next = if delta.x < 0.0 {
                        if self.tab == TABS.len() - 1 {
                            SHELF_TAB
                        } else {
                            self.tab + 1
                        }
                    } else if self.tab == SHELF_TAB {
                        TABS.len() - 1
                    } else {
                        self.tab - 1
                    };
                    if self.tab == TABS.len() - 1 && next == SHELF_TAB {
                        self.transition.start_with_direction(self.tab, next, 1.0);
                        self.tab = next;
                    } else if self.tab == SHELF_TAB && next == TABS.len() - 1 {
                        self.transition.start_with_direction(self.tab, next, -1.0);
                        self.tab = next;
                    } else {
                        self.set_tab(next);
                    }
                }
            } else {
                self.page_swipe_start = None;
            }
        }
    }

    /// Removes the title bar for good, and keeps it removed.
    ///
    /// `with_decorations(false)` alone is not enough on Windows: winit keeps `WS_CAPTION`,
    /// `WS_SYSMENU` and the border bits in the window style and only hides the non-client
    /// area by faking `WM_NCCALCSIZE`. Anything that forces a frame recalculation or
    /// repaints the non-client area (a new window region, activation, `SWP_FRAMECHANGED`)
    /// can then flash the native close/minimise/maximise buttons for a frame.
    ///
    /// winit rewrites the whole style from its own flag set every time a window flag
    /// changes (`set_visible` after the first frame, always-on-top, ...), which brings the
    /// caption back. So this is re-checked every frame and only touches the window when
    /// the bits have actually returned.
    fn strip_native_caption(&mut self) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GWL_EXSTYLE, GWL_STYLE, GetWindowLongPtrW, SWP_FRAMECHANGED, SWP_NOACTIVATE,
            SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, WS_BORDER,
            WS_CAPTION, WS_EX_DLGMODALFRAME, WS_EX_WINDOWEDGE, WS_MAXIMIZEBOX, WS_MINIMIZEBOX,
            WS_SIZEBOX, WS_SYSMENU,
        };

        let Some(hwnd) = self.hwnd else { return };
        const FRAME_BITS: u32 =
            WS_CAPTION | WS_SYSMENU | WS_SIZEBOX | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_BORDER;
        const FRAME_BITS_EX: u32 = WS_EX_DLGMODALFRAME | WS_EX_WINDOWEDGE;

        // SAFETY: `hwnd` is this process's live window; the masks only clear caption and
        // border bits, and the frameless flush keeps position, size and z-order.
        unsafe {
            let style = GetWindowLongPtrW(hwnd as _, GWL_STYLE) as u32;
            let ex_style = GetWindowLongPtrW(hwnd as _, GWL_EXSTYLE) as u32;
            let dirty = style & FRAME_BITS != 0 || ex_style & FRAME_BITS_EX != 0;
            if !dirty {
                return;
            }

            SetWindowLongPtrW(hwnd as _, GWL_STYLE, (style & !FRAME_BITS) as isize);
            SetWindowLongPtrW(hwnd as _, GWL_EXSTYLE, (ex_style & !FRAME_BITS_EX) as isize);
            SetWindowPos(
                hwnd as _,
                std::ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    fn update_native_region(&self, island: Rect, trigger: Rect, pixels_per_point: f32) {
        use windows_sys::Win32::Graphics::Gdi::{
            CombineRgn, CreateRoundRectRgn, DeleteObject, RGN_OR, SetWindowRgn,
        };

        let Some(hwnd) = self.hwnd else { return };
        let scale = |value: f32| (value * pixels_per_point).round() as i32;
        let round_rect = |rect: Rect, radius: f32| unsafe {
            CreateRoundRectRgn(
                scale(rect.left()),
                scale(rect.top()),
                scale(rect.right()).max(scale(rect.left()) + 1),
                scale(rect.bottom()).max(scale(rect.top()) + 1),
                scale(radius * 2.0).max(1),
                scale(radius * 2.0).max(1),
            )
        };

        let t = ((island.height() - ui::IDLE_SIZE.y) / (ui::EXPANDED_SIZE.y - ui::IDLE_SIZE.y))
            .clamp(0.0, 1.0);
        let radius =
            egui::lerp(ui::IDLE_SIZE.y * 0.5..=30.0, t).min(island.size().min_elem() * 0.5);

        let rect_key = |rect: Rect| {
            [
                scale(rect.left()),
                scale(rect.top()),
                scale(rect.right()),
                scale(rect.bottom()),
            ]
        };

        // Every rectangle that contributes to the region, in physical pixels. Comparing this
        // against the applied region avoids calling `SetWindowRgn` on every single frame: each
        // call invalidates the window and can make Windows repaint a native frame.
        let toast = self
            .notifications
            .front()
            .filter(|note| note.age() > 0.35)
            .map(|note| {
                let pop = ((note.age() - 0.35) / 0.35).clamp(0.0, 1.0);
                let eased = pop * pop * (3.0 - 2.0 * pop);
                let width = island.width().max(280.0);
                Rect::from_min_size(
                    egui::pos2(
                        island.center().x - width * 0.5,
                        island.bottom() + 2.0 - (1.0 - eased) * 8.0,
                    ),
                    egui::vec2(width, 58.0),
                )
            });

        let key = [
            rect_key(island),
            rect_key(trigger),
            toast.map_or([i32::MAX; 4], rect_key),
        ];
        if self.applied_region.borrow().as_ref() == Some(&key) {
            return;
        }

        let base = round_rect(island, radius);
        if base.is_null() {
            return;
        }

        let trigger_region = round_rect(trigger, 5.0);
        if !trigger_region.is_null() {
            // SAFETY: Both regions are valid GDI regions owned by this function.
            unsafe {
                CombineRgn(base, base, trigger_region, RGN_OR);
                DeleteObject(trigger_region);
            }
        }

        if let Some(toast) = toast {
            let toast_region = round_rect(toast, 16.0);
            if !toast_region.is_null() {
                // SAFETY: Both regions are valid GDI regions owned by this function.
                unsafe {
                    CombineRgn(base, base, toast_region, RGN_OR);
                    DeleteObject(toast_region);
                }
            }
        }

        // `redraw = 0`: egui repaints the island itself. Letting Windows repaint here also
        // invalidated the non-client area, which is what flashed the native caption away.
        // SAFETY: hwnd is the live window handle captured from CreationContext. On success,
        // Windows takes ownership of `base`; otherwise this function releases it.
        let applied = unsafe { SetWindowRgn(hwnd as _, base, 0) };
        if applied == 0 {
            unsafe { DeleteObject(base) };
        } else {
            // A changed region still needs one repaint for the area it revealed.
            // SAFETY: hwnd is live and the null rect means "whole (client) window".
            unsafe {
                windows_sys::Win32::Graphics::Gdi::InvalidateRect(hwnd as _, std::ptr::null(), 0);
            }
            *self.applied_region.borrow_mut() = Some(key);
        }
    }
}

impl DynamicIsland {
    fn handle_shortcuts(&mut self, ui: &egui::Ui) {
        let (space, open, escape) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::Space),
                i.key_pressed(egui::Key::O) && i.modifiers.command,
                i.key_pressed(egui::Key::Escape),
            )
        });

        if escape && self.expanded {
            self.close();
        }
        if space && self.expanded {
            match self.tab {
                SHELF_TAB => {}
                // `Space` does not run a command: launching a dev server by accident is far
                // more costly than an extra click, so only the row's run button does that.
                SHORTCUTS_TAB => {}
                MUSIC_TAB => {
                    if self.system_media.is_some() {
                        self.media_toggle();
                    } else {
                        self.audio.toggle();
                    }
                }
                TIMER_TAB => self.timer.toggle(),
                _ => {}
            }
        }
        if open
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("Audio", &["mp3", "wav", "flac", "ogg", "m4a", "mp4"])
                .pick_file()
        {
            self.audio.load(path);
            self.open();
            self.set_tab(MUSIC_TAB);
        }
    }

    fn idle_view(&mut self, ui: &mut egui::Ui, outer: Rect) {
        let row = Rect::from_center_size(
            outer.center(),
            egui::vec2((outer.width() - 28.0).max(0.0), outer.height()),
        );
        let mut row_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(row)
                .layout(Layout::left_to_right(Align::Center)),
        );

        let (playing, title) = if let Some(media) = self.system_media.as_ref() {
            (media.playing, media.title.clone())
        } else {
            (
                self.audio.is_playing(),
                self.audio
                    .track()
                    .map_or("Music", |t| t.title.as_str())
                    .into(),
            )
        };
        let shelved = self.shelf.len();
        let alerts = self.notifications.len();
        let mut suffix = String::new();
        let mut suffix_color = ui::MUTED;
        if self.timer.state() == TimerState::Running {
            suffix = self.timer.label_time();
            suffix_color = ui::ACCENT;
        }
        if shelved > 0 {
            if !suffix.is_empty() {
                suffix.push_str("  ·  ");
            }
            suffix.push_str(&match shelved {
                1 => "1 file".to_owned(),
                n => format!("{n} files"),
            });
            suffix_color = ui::ACCENT;
        } else if alerts > 0 {
            if !suffix.is_empty() {
                suffix.push_str("  ·  ");
            }
            suffix.push_str(&format!("{alerts} alerts"));
        }

        let painter = row_ui.painter().clone();
        let title_font = eframe::egui::FontId::proportional(13.0);
        let title_galley = painter.layout_no_wrap(title, title_font, ui::TEXT);
        let suffix_galley = (!suffix.is_empty()).then(|| {
            painter.layout_no_wrap(
                suffix.clone(),
                eframe::egui::FontId::proportional(11.0),
                suffix_color,
            )
        });
        let icon_width = if playing { 23.0 } else { 0.0 };
        let suffix_width = suffix_galley.as_ref().map_or(0.0, |g| g.size().x + 10.0);
        let title_width = (row_ui.available_width() - icon_width - suffix_width).max(0.0);
        if playing {
            ui::equalizer(&mut row_ui, true, 14.0);
            row_ui.add_space(9.0);
        }
        let (title_rect, _) = row_ui.allocate_exact_size(
            Vec2::new(title_width, 20.0),
            Sense::hover(),
        );
        if title_galley.size().x > title_rect.width() && title_rect.width() > 0.0 {
            let cycle = title_galley.size().x + 24.0;
            let offset = (ui.ctx().input(|i| i.time) as f32 % 8.0) / 8.0 * cycle;
            let clip = painter.with_clip_rect(title_rect);
            let y = title_rect.center().y - title_galley.size().y * 0.5;
            clip.galley(
                egui::pos2(title_rect.left() - offset, y),
                title_galley.clone(),
                ui::TEXT,
            );
            clip.galley(
                egui::pos2(title_rect.left() - offset + cycle, y),
                title_galley,
                ui::TEXT,
            );
            row_ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
        } else {
            painter.with_clip_rect(title_rect).galley(
                egui::pos2(title_rect.left(), title_rect.center().y - title_galley.size().y * 0.5),
                title_galley,
                ui::TEXT,
            );
        }
        if let Some(galley) = suffix_galley {
            row_ui.add_space(10.0);
            row_ui.painter().galley(
                egui::pos2(row_ui.cursor().min.x, title_rect.center().y - galley.size().y * 0.5),
                galley,
                suffix_color,
            );
        }
    }

    fn expanded_view(&mut self, ui: &mut egui::Ui) {
        // `horizontal()` and `with_layout(right_to_left)` both lay children out from the left
        // edge of the space that is left, so a right-aligned close button would end up past
        // the island padding. Right blocks therefore get their own `Ui` that ends exactly on
        // the padded right edge, and text is truncated to the room it is given.
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.set_width(ui.available_width());

        let close = 24.0;
        let header = ui.available_width().max(0.0);
        let (header_rect, _) = ui.allocate_exact_size(Vec2::new(header, 28.0), Sense::hover());

        let mut left = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_size(
                    header_rect.min,
                    Vec2::new((header - close).max(0.0), header_rect.height()),
                ))
                .layout(Layout::left_to_right(Align::Center)),
        );
        left.spacing_mut().item_spacing.x = 0.0;
        ui::equalizer(
            &mut left,
            self.audio.is_playing() || self.system_media.as_ref().is_some_and(|m| m.playing),
            13.0,
        );
        left.add_space(10.0);
        left.vertical(|ui| {
            ui::label(ui, "ISLAND", 10.0, ui::MUTED);
            ui::label(
                ui,
                match self.tab {
                    SHELF_TAB => "Temporary shelf",
                    SHORTCUTS_TAB => "Shortcuts",
                    MUSIC_TAB => "Now playing",
                    TIMER_TAB => "Focus timer",
                    ALERTS_TAB => "Inbox",
                    SETTINGS_TAB => "Settings",
                    _ => "Settings",
                },
                14.0,
                ui::TEXT,
            );
        });

        let mut right = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_max(
                    egui::pos2(header_rect.right() - close, header_rect.top()),
                    header_rect.right_bottom(),
                ))
                .layout(Layout::right_to_left(Align::Center)),
        );
        right.spacing_mut().item_spacing.x = 0.0;
        if ui::icon_button(&mut right, "\u{00d7}", close).clicked() {
            self.close();
        }
        ui.add_space(14.0);

        self.nav_row(ui);
        ui.add_space(16.0);

        self.content(ui);
    }

    /// Draws the current page, sliding it in over the previous one while a transition runs.
    ///
    /// Both pages are drawn into the same fixed-size area and clipped to the island, so the
    /// layout never reflows mid-slide. The outgoing page fades and drifts the other way to
    /// give the motion some depth.
    fn content(&mut self, ui: &mut egui::Ui) {
        let area = Rect::from_min_size(ui.cursor().min, ui.available_size());
        let progress = self.transition.progress();

        // Idle: just draw the current page.
        if progress >= 1.0 {
            self.transition.finish();
            let mut page = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(area)
                    .layout(Layout::top_down(Align::Min)),
            );
            page.set_clip_rect(area);
            self.page(&mut page, self.tab);
            ui.advance_cursor_after_rect(area);
            return;
        }

        // Ease-out so the page decelerates into place instead of stopping abruptly.
        let eased = 1.0 - (1.0 - progress).powi(3);
        let direction = self.transition.direction();
        let travel = area.width() + ui::PAD;
        let outgoing = self.transition.outgoing();
        let incoming = self.transition.incoming();

        let mut layer = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(area)
                .layout(Layout::top_down(Align::Min)),
        );
        layer.set_clip_rect(area);

        // Outgoing page drifts a little the other way and dims out. It is disabled so a click
        // during the slide cannot hit a control that is already on its way off screen.
        let mut old = layer.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_size(
                    area.min - Vec2::new(direction * travel * eased * 0.35, 0.0),
                    area.size(),
                ))
                .layout(Layout::top_down(Align::Min)),
        );
        old.set_clip_rect(area);
        // `disable()` also dims via the painter; reset that so the slide fade is the only
        // opacity effect and the page does not double-darken.
        old.disable();
        old.set_opacity(1.0 - eased);
        self.page(&mut old, outgoing);

        // Incoming page slides in from the far edge on top.
        let mut new = layer.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_size(
                    area.min + Vec2::new(direction * travel * (1.0 - eased), 0.0),
                    area.size(),
                ))
                .layout(Layout::top_down(Align::Min)),
        );
        new.set_clip_rect(area);
        // The incoming page stays opaque: it slides over the outgoing one rather than fading,
        // which reads as a real page turn instead of a cross-dissolve.
        self.page(&mut new, incoming);

        ui.advance_cursor_after_rect(area);
    }

    fn page(&mut self, ui: &mut egui::Ui, tab: usize) {
        match tab {
            SHELF_TAB => panels::shelf_tab(self, ui),
            SHORTCUTS_TAB => panels::shortcuts_tab(self, ui),
            MUSIC_TAB => panels::music_tab(self, ui),
            TIMER_TAB => panels::timer_tab(self, ui),
            ALERTS_TAB => panels::alerts_tab(self, ui),
            SETTINGS_TAB => panels::settings_tab(ui),
            _ => panels::alerts_tab(self, ui),
        }
    }

    /// Minimal page navigation: a chevron on each side, the current page name centred.
    fn nav_row(&mut self, ui: &mut egui::Ui) {
        let width = ui.available_width().max(0.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 26.0), Sense::hover());
        let button = 26.0;

        let mut left = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_size(rect.min, Vec2::new(button, rect.height())))
                .layout(Layout::left_to_right(Align::Center)),
        );
        if ui::nav_button(&mut left, ui::Icon::ArrowLeft, button).clicked() {
            self.step_tab(-1);
        }

        let mut right = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_max(
                    egui::pos2(rect.right() - button, rect.top()),
                    rect.right_bottom(),
                ))
                .layout(Layout::right_to_left(Align::Center)),
        );
        if ui::nav_button(&mut right, ui::Icon::ArrowRight, button).clicked() {
            self.step_tab(1);
        }

        // The label is centred in the gap between the two buttons and clipped to it.
        let gap = (width - button * 2.0 - 16.0).max(0.0);
        let middle = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_center_size(rect.center(), Vec2::new(gap, rect.height())))
                .layout(Layout::left_to_right(Align::Center)),
        );
        let label = TABS[self.tab.min(TABS.len() - 1)];
        let accent = match self.tab {
            SHELF_TAB => ui::SHELF_ACCENT,
            SHORTCUTS_TAB => ui::SHORTCUT_ACCENT,
            MUSIC_TAB => ui::MUSIC_ACCENT,
            TIMER_TAB => ui::TIMER_ACCENT,
            ALERTS_TAB => ui::ALERT_ACCENT,
            _ => ui::ACCENT,
        };
        let galley = ui::fit_to(&middle, label, &egui::FontId::proportional(11.0), accent, gap);
        let pos = egui::pos2(
            middle.max_rect().center().x - galley.size().x * 0.5,
            middle.max_rect().center().y - galley.size().y * 0.5,
        );
        middle.painter().galley(pos, galley, accent);
    }

    fn floating_notification(&mut self, ui: &mut egui::Ui, island: Rect) {
        let Some(note) = self.notifications.front() else {
            return;
        };
        let view = NotificationView::from(note);
        if panels::floating_note(ui, island, &view) {
            self.open();
            self.set_tab(ALERTS_TAB);
        }
    }

    /// Shelves files dropped onto the island and switches to the shelf so the user sees them.
    fn take_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<std::path::PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .collect()
        });
        let mut added = 0;
        for path in dropped {
            if self.shelf.add(path).is_some() {
                added += 1;
            }
        }
        if added > 0 {
            self.open();
            self.set_tab(SHELF_TAB);
        }
    }
}
