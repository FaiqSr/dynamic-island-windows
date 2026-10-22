//! Real file icons for the shelf and the shortcut grid.
//!
//! A desktop-like grid needs the icon Windows would show for a file, which lives in the shell
//! (`SHGetFileInfoW`). For a shortcut whose command starts with an executable, that executable is
//! queried instead, so a shortcut to `chrome` shows Chrome rather than a generic document.
//!
//! No new dependency: the icon comes back as an `HICON`, is copied into a 32-bit DIB with GDI,
//! and is handed to egui as straight RGBA. Non-Windows builds return `None` and callers draw a
//! monogram tile instead.

use std::path::Path;

#[cfg(windows)]
pub use platform::IconPixels;

/// Icons for `path`. `command` is the shortcut's command line, used to prefer the icon of the
/// executable it launches. `None` when the platform or the shell cannot supply one.
#[cfg(windows)]
pub fn for_path(path: &Path, command: Option<&str>) -> Option<IconPixels> {
    platform::for_path(path, command)
}

#[cfg(not(windows))]
pub fn for_path(_path: &Path, _command: Option<&str>) -> Option<IconPixels> {
    None
}

#[cfg(windows)]
mod platform {
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::path::{Path, PathBuf};

    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC,
        GetDIBits, GetObjectW, HBITMAP, ReleaseDC,
    };
    use windows::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_NORMAL, FILE_FLAGS_AND_ATTRIBUTES,
    };
    use windows::Win32::UI::Shell::{
        SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGFI_USEFILEATTRIBUTES, SHGetFileInfoW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};
    use windows::core::PCWSTR;

    /// A decoded icon, ready to be uploaded as a texture.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct IconPixels {
        pub size: u32,
        /// Straight RGBA, row-major, top-down.
        pub rgba: Vec<u8>,
    }

    impl IconPixels {
        /// `(width, height)`, derived from the buffer so callers never have to assume square.
        pub fn dimensions(&self) -> [usize; 2] {
            let width = self.size.max(1) as usize;
            [width, (self.rgba.len() / 4 / width).max(1)]
        }
    }

    /// Largest edge accepted, so a malformed bitmap cannot ask for a huge allocation.
    const MAX_EDGE: u32 = 512;

    pub fn for_path(path: &Path, command: Option<&str>) -> Option<IconPixels> {
        // The executable named in the command is the most specific icon available.
        if let Some(exe) = command.and_then(executable_in)
            && let Some(icon) = shell_icon(&exe, 0)
        {
            return Some(icon);
        }
        if let Some(icon) = shell_icon(path, 0) {
            return Some(icon);
        }
        // Extension-only lookup: answers for files that are gone, and for data files the shell
        // only knows by extension.
        let ext = path.extension()?.to_string_lossy().to_lowercase();
        if ext.is_empty() {
            return None;
        }
        shell_icon(Path::new(&format!("dummy.{ext}")), FILE_ATTRIBUTE_NORMAL.0)
    }
    /// Executable (or script) named at the start of a command line.
    fn executable_in(command: &str) -> Option<PathBuf> {
        let command = command.trim();
        let token = if let Some(rest) = command.strip_prefix('"') {
            rest.split('"').next().unwrap_or("")
        } else {
            command.split_whitespace().next().unwrap_or("")
        };
        let path = Path::new(token);
        // `npm`, `code` and friends are not files on disk and have no icon to read.
        if !path.is_file() {
            return None;
        }
        let ext = path.extension()?.to_string_lossy().to_lowercase();
        matches!(ext.as_str(), "exe" | "lnk" | "bat" | "cmd" | "ico").then(|| path.to_path_buf())
    }

    fn wide(path: &Path) -> Vec<u16> {
        path.to_string_lossy().encode_utf16().chain([0]).collect()
    }

    /// `SHGetFileInfoW` for one large icon. A non-zero `attributes` selects the
    /// `USEFILEATTRIBUTES` path, which answers from the extension table without touching disk.
    fn shell_icon(path: &Path, attributes: u32) -> Option<IconPixels> {
        let name = wide(path);
        let mut info = SHFILEINFOW::default();
        let mut flags = SHGFI_ICON | SHGFI_LARGEICON;
        if attributes != 0 {
            flags |= SHGFI_USEFILEATTRIBUTES;
        }
        // SAFETY: `name` is a null-terminated UTF-16 buffer and `info` is a correctly sized,
        // initialised out-parameter; both outlive the call.
        let ok = unsafe {
            SHGetFileInfoW(
                PCWSTR(name.as_ptr()),
                FILE_FLAGS_AND_ATTRIBUTES(attributes),
                Some(&mut info),
                size_of::<SHFILEINFOW>() as u32,
                flags,
            )
        };
        if ok == 0 || info.hIcon.is_invalid() {
            return None;
        }
        let pixels = icon_to_rgba(info.hIcon);
        // SAFETY: The shell created this handle for us and nobody else holds it. It must be
        // destroyed even when decoding failed, or every repaint would leak one.
        let _ = unsafe { DestroyIcon(info.hIcon) };
        pixels
    }

    fn icon_to_rgba(icon: HICON) -> Option<IconPixels> {
        let mut info = ICONINFO::default();
        // SAFETY: `icon` is a live HICON and `info` is a valid out-parameter.
        unsafe { GetIconInfo(icon, &mut info) }.ok()?;
        // A monochrome icon has no colour bitmap; the mask alone is not worth drawing.
        let pixels = (!info.hbmColor.is_invalid()).then(|| bitmap_to_rgba(info.hbmColor));
        // SAFETY: Both bitmaps belong to this call; GetIconInfo hands them to the caller.
        unsafe {
            if !info.hbmColor.is_invalid() {
                let _ = DeleteObject(info.hbmColor.into());
            }
            if !info.hbmMask.is_invalid() {
                let _ = DeleteObject(info.hbmMask.into());
            }
        }
        pixels.flatten()
    }

    /// Copies a colour bitmap into straight, top-down RGBA.
    fn bitmap_to_rgba(bitmap: HBITMAP) -> Option<IconPixels> {
        let mut bmp = BITMAP::default();
        // SAFETY: `bitmap` is live and `bmp` is a correctly sized out-parameter.
        let read = unsafe {
            GetObjectW(
                bitmap.into(),
                size_of::<BITMAP>() as i32,
                Some(&mut bmp as *mut _ as *mut c_void),
            )
        };
        if read == 0 {
            return None;
        }
        let width = u32::try_from(bmp.bmWidth).ok()?.min(MAX_EDGE);
        let height = u32::try_from(bmp.bmHeight).ok()?.min(MAX_EDGE);
        if width == 0 || height == 0 {
            return None;
        }

        // SAFETY: A screen DC is borrowed only for this call and released below; `DeleteDC` is
        // wrong here because the DC belongs to the window station.
        let dc = unsafe { GetDC(None) };
        if dc.is_invalid() {
            return None;
        }

        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                // Negative height asks for a top-down scan, matching the RGBA order egui expects.
                biHeight: -(height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut buffer = vec![0u8; (width * 4 * height) as usize];
        // SAFETY: `bmi` describes `buffer` exactly and both outlive the call.
        let lines = unsafe {
            GetDIBits(
                dc,
                bitmap,
                0,
                height,
                Some(buffer.as_mut_ptr() as *mut c_void),
                &mut bmi,
                DIB_RGB_COLORS,
            )
        };
        // SAFETY: `dc` came from GetDC and is released exactly once.
        unsafe { ReleaseDC(None, dc) };
        if lines == 0 {
            return None;
        }

        // GDI returns BGRA. Icons without an alpha channel come back with a zero alpha, which
        // egui would draw fully transparent; the mask was already applied, so treat them opaque.
        let (pixels, _) = buffer.as_chunks_mut::<4>();
        let opaque = pixels.iter().all(|px| px[3] == 0);
        for px in pixels {
            px.swap(0, 2);
            if opaque {
                px[3] = 255;
            }
        }

        Some(IconPixels {
            size: width,
            rgba: buffer,
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn an_executable_named_in_a_command_is_detected() {
            // `cmd.exe` is on every Windows install; the quoted form must be unwrapped too.
            let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
            let cmd = format!("{system_root}\\System32\\cmd.exe");
            assert_eq!(
                executable_in(&format!("\"{cmd}\" /c echo hi")),
                Some(PathBuf::from(&cmd))
            );
            assert_eq!(executable_in("npm run dev"), None);
        }

        #[test]
        fn a_path_is_wide_and_null_terminated() {
            let w = wide(Path::new("a.txt"));
            assert_eq!(w.last(), Some(&0));
            assert_eq!(w.len(), "a.txt".len() + 1);
        }

        /// Exercises the real shell path: decode an icon and check it is a plausible RGBA image.
        /// Skipped silently on a machine with no shell icons (a bare CI box).
        #[test]
        fn the_shell_returns_a_usable_icon() {
            let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
            let notepad = Path::new(&system_root).join("System32\\notepad.exe");
            if !notepad.is_file() {
                return;
            }
            let Some(icon) = for_path(&notepad, None) else {
                return;
            };
            let [width, height] = icon.dimensions();
            assert!(width > 0 && height > 0);
            assert_eq!(icon.rgba.len(), width * height * 4);
            // A blank icon would mean the decode silently produced nothing; a real executable
            // icon has at least some visible pixels.
            let (pixels, _) = icon.rgba.as_chunks::<4>();
            assert!(pixels.iter().any(|px| px[3] > 0));
        }
    }
}
