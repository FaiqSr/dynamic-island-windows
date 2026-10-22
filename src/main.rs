// The GUI app has no use for a console window on Windows. Release builds use the
// `windows` subsystem; debug builds keep the console but hide it at startup so
// `cargo run` does not leave a terminal on screen.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod animation;
mod app;
mod audio;
mod icons;
mod media;
mod notifications;
mod shortcuts;
mod storage;
mod timer;
mod ui;
mod volume;

use std::time::Duration;

use eframe::egui;

fn main() -> eframe::Result<()> {
    #[cfg(all(windows, debug_assertions))]
    hide_console();

    #[cfg(windows)]
    let position = {
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN};
        let screen_width = unsafe { GetSystemMetrics(SM_CXSCREEN) } as f32;
        egui::pos2((screen_width - ui::EXPANDED_SIZE.x) * 0.5, 0.0)
    };
    #[cfg(not(windows))]
    let position = egui::pos2(0.0, 0.0);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Dynamic Island")
            .with_inner_size(ui::EXPANDED_SIZE)
            .with_min_inner_size(ui::IDLE_SIZE)
            .with_transparent(true)
            .with_decorations(false)
            .with_always_on_top()
            .with_taskbar(false)
            .with_resizable(false)
            // The island has its own close button; the native caption menu must not appear
            // either, so no window buttons at all.
            .with_close_button(false)
            .with_minimize_button(false)
            .with_maximize_button(false)
            .with_position(position)
            .with_has_shadow(false),
        ..Default::default()
    };

    eframe::run_native(
        "Dynamic Island",
        options,
        Box::new(|cc| {
            cc.egui_ctx.all_styles_mut(|style| {
                style.spacing.window_margin = egui::Margin::same(0);
            });
            Ok(Box::new(app::DynamicIsland::new(cc)))
        }),
    )
}

#[cfg(all(windows, debug_assertions))]
fn hide_console() {
    use windows_sys::Win32::System::Console::{GetConsoleWindow, GetStdHandle, STD_OUTPUT_HANDLE};
    use windows_sys::Win32::UI::WindowsAndMessaging::{SW_HIDE, ShowWindow};

    // SAFETY: Both calls only read the console attached to this process and show or hide it.
    unsafe {
        let hwnd = GetConsoleWindow();
        if !hwnd.is_null() {
            ShowWindow(hwnd, SW_HIDE);
        }
        // Writes from a hidden console handle are discarded instead of erroring.
        let _ = GetStdHandle(STD_OUTPUT_HANDLE);
    }
}

/// Frame pacing: 60 fps while animating, calmer when the UI is at rest.
pub const ACTIVE_FRAME: Duration = Duration::from_millis(16);
pub const IDLE_FRAME: Duration = Duration::from_millis(50);
