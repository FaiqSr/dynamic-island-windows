//! A temporary shelf for files.
//!
//! The point is to break the "alt-tab, find folder, find app, drag" loop: files are
//! dropped onto the island (or picked with `+`), held in memory-free references, and
//! can then be dragged *out* of the island straight into another app such as WhatsApp.
//!
//! Files are never copied or moved on disk. A shelf entry is just a path, so originals
//! stay where they are and the shelf can be cleared at any time without data loss.

use std::path::PathBuf;

/// Most files a shelf will hold; beyond this the oldest is dropped.
const CAPACITY: usize = 12;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub path: PathBuf,
    pub name: String,
    /// Bytes, when the file could be stat'ed.
    pub size: Option<u64>,
}

impl Item {
    fn new(path: PathBuf) -> Self {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let size = std::fs::metadata(&path).ok().map(|meta| meta.len());
        Self { path, name, size }
    }

    /// True when the path still exists, so stale entries can be pruned.
    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    pub fn extension(&self) -> String {
        self.path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    }

    /// Short human-readable size, e.g. `4.2 MB`.
    pub fn size_label(&self) -> String {
        match self.size {
            Some(bytes) => human_size(bytes),
            None => "--".into(),
        }
    }
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

pub struct Shelf {
    items: Vec<Item>,
    error: Option<String>,
}

impl Shelf {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            error: None,
        }
    }

    pub fn items(&self) -> &[Item] {
        &self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Adds a file, ignoring duplicates and anything that is not a readable file.
    /// Returns the display name when the file was actually added.
    pub fn add(&mut self, path: PathBuf) -> Option<String> {
        let meta = match std::fs::metadata(&path) {
            Ok(meta) => meta,
            Err(_) => {
                self.error = Some(format!("Cannot read \"{}\"", path.display()));
                return None;
            }
        };
        if !meta.is_file() {
            self.error = Some("Only files can be shelved".into());
            return None;
        }
        if self.items.iter().any(|item| item.path == path) {
            return None;
        }
        let item = Item::new(path);
        let name = item.name.clone();
        self.items.push(item);
        while self.items.len() > CAPACITY {
            self.items.remove(0);
        }
        self.error = None;
        Some(name)
    }

    /// Removes the entry at `index`. The grid uses [`Shelf::dismiss_path`]; this stays for the
    /// storage tests, which reason in positions.
    #[cfg(test)]
    pub fn dismiss(&mut self, index: usize) {
        if index < self.items.len() {
            self.items.remove(index);
        }
    }

    /// Same as [`Shelf::dismiss`], for callers that hold a path rather than a live index.
    pub fn dismiss_path(&mut self, path: &std::path::Path) {
        self.items.retain(|item| item.path != path);
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.error = None;
    }

    /// Drops entries whose file was moved or deleted elsewhere while shelved.
    pub fn prune(&mut self) {
        self.items.retain(Item::exists);
    }
}

/// Hands `paths` to the OS drag-and-drop loop so they can be dropped onto another app.
///
/// This blocks until the user releases the mouse or presses Escape, and must therefore be
/// called from the UI thread. Returns `true` when the drop landed on a target.
///
/// Non-Windows builds have no drag-out yet and return `false`.
pub fn drag_out(paths: &[PathBuf]) -> bool {
    if paths.is_empty() {
        return false;
    }
    platform::drag_out(paths)
}

#[cfg(windows)]
mod platform {
    use std::cell::RefCell;
    use std::path::PathBuf;

    use windows::Win32::Foundation::{
        DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, DV_E_FORMATETC, E_NOTIMPL,
        GlobalFree, HGLOBAL, OLE_E_ADVISENOTSUPPORTED, S_OK,
    };
    use windows::Win32::System::Com::{
        FORMATETC, IAdviseSink, IDataObject, IDataObject_Impl, IEnumFORMATETC, IEnumSTATDATA,
        STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL,
    };
    use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
    use windows::Win32::System::Ole::{
        DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_MOVE, DoDragDrop, IDropSource, IDropSource_Impl,
        OleInitialize,
    };
    use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
    use windows::Win32::UI::Shell::DROPFILES;
    use windows::core::{BOOL, Error, HRESULT, Ref, Result, implement};

    /// `CF_HDROP`, the clipboard format Windows uses for a list of files.
    const CF_HDROP: u16 = 15;

    thread_local! {
        /// The `HGLOBAL` currently being offered, so `GetData` can hand it to the target.
        /// `DoDragDrop` runs on the UI thread and is synchronous, so a thread-local is enough.
        static DROP_HANDLE: RefCell<Option<HGLOBAL>> = const { RefCell::new(None) };
    }

    /// Builds the `DROPFILES` blob: a header followed by double-NUL-terminated UTF-16 paths.
    fn build_dropfiles(paths: &[PathBuf]) -> Option<HGLOBAL> {
        let mut wide: Vec<u16> = Vec::new();
        for path in paths {
            wide.extend(path.to_string_lossy().encode_utf16());
            wide.push(0);
        }
        wide.push(0);

        let header = std::mem::size_of::<DROPFILES>();
        let bytes = header + wide.len() * 2;
        // SAFETY: The allocation is sized to fit the header plus every path, and the copy
        // writes exactly `wide.len() * 2` bytes starting after the header.
        unsafe {
            let handle = GlobalAlloc(GMEM_MOVEABLE, bytes).ok()?;
            let base = GlobalLock(handle) as *mut u8;
            if base.is_null() {
                let _ = GlobalFree(Some(handle));
                return None;
            }
            let drop = base as *mut DROPFILES;
            (*drop).pFiles = header as u32;
            (*drop).fWide = BOOL(1);
            std::ptr::copy_nonoverlapping(wide.as_ptr().cast::<u8>(), base.add(header), wide.len() * 2);
            let _ = GlobalUnlock(handle);
            Some(handle)
        }
    }

    #[implement(IDataObject)]
    struct ShelfData;

    impl IDataObject_Impl for ShelfData_Impl {
        fn GetData(&self, format: *const FORMATETC) -> Result<STGMEDIUM> {
            // Every other format is unsupported; the target only ever asks for CF_HDROP.
            let handle = DROP_HANDLE
                .with(|slot| *slot.borrow())
                .ok_or_else(|| Error::from(DV_E_FORMATETC))?;
            // SAFETY: `format` is provided by the drop target for the duration of the call.
            if unsafe { (*format).cfFormat } != CF_HDROP {
                return Err(Error::from(DV_E_FORMATETC));
            }
            Ok(STGMEDIUM {
                tymed: TYMED_HGLOBAL.0 as u32,
                u: STGMEDIUM_0 { hGlobal: handle },
                // The caller owns the handle after the drop; `FGDI`/fRelease semantics of
                // the caller decide, so no release interface is attached.
                pUnkForRelease: std::mem::ManuallyDrop::new(None),
            })
        }

        fn GetDataHere(&self, _format: *const FORMATETC, _medium: *mut STGMEDIUM) -> Result<()> {
            Err(Error::from(E_NOTIMPL))
        }

        fn QueryGetData(&self, format: *const FORMATETC) -> HRESULT {
            // SAFETY: `format` is valid for the duration of the call.
            if unsafe { (*format).cfFormat } == CF_HDROP {
                S_OK
            } else {
                DV_E_FORMATETC
            }
        }

        fn GetCanonicalFormatEtc(
            &self,
            _format: *const FORMATETC,
            out: *mut FORMATETC,
        ) -> HRESULT {
            // SAFETY: `out` is an out-parameter supplied by the caller.
            unsafe { (*out).ptd = std::ptr::null_mut() };
            windows::Win32::Foundation::DATA_S_SAMEFORMATETC
        }

        fn SetData(
            &self,
            _format: *const FORMATETC,
            _medium: *const STGMEDIUM,
            _release: BOOL,
        ) -> Result<()> {
            Err(Error::from(E_NOTIMPL))
        }

        fn EnumFormatEtc(&self, _direction: u32) -> Result<IEnumFORMATETC> {
            Err(Error::from(OLE_E_ADVISENOTSUPPORTED))
        }

        fn DAdvise(
            &self,
            _format: *const FORMATETC,
            _flags: u32,
            _sink: Ref<'_, IAdviseSink>,
        ) -> Result<u32> {
            Err(Error::from(OLE_E_ADVISENOTSUPPORTED))
        }

        fn DUnadvise(&self, _connection: u32) -> Result<()> {
            Err(Error::from(OLE_E_ADVISENOTSUPPORTED))
        }

        fn EnumDAdvise(&self) -> Result<IEnumSTATDATA> {
            Err(Error::from(OLE_E_ADVISENOTSUPPORTED))
        }
    }

    #[implement(IDropSource)]
    struct ShelfSource;

    impl IDropSource_Impl for ShelfSource_Impl {
        fn QueryContinueDrag(&self, escape_pressed: BOOL, key_state: MODIFIERKEYS_FLAGS) -> HRESULT {
            if escape_pressed.as_bool() {
                return DRAGDROP_S_CANCEL;
            }
            // The button being released is what ends the drag with a drop.
            if key_state.0 & MK_LBUTTON.0 == 0 {
                return DRAGDROP_S_DROP;
            }
            S_OK
        }

        fn GiveFeedback(&self, _effect: DROPEFFECT) -> HRESULT {
            DRAGDROP_S_USEDEFAULTCURSORS
        }
    }

    pub fn drag_out(paths: &[PathBuf]) -> bool {
        // SAFETY: Initialising OLE with the apartment already used by the WinRT media
        // session is idempotent; the result is ignored on purpose so an already-initialised
        // apartment does not abort the drag.
        unsafe {
            let _ = OleInitialize(None);
        }

        let Some(handle) = build_dropfiles(paths) else {
            return false;
        };
        DROP_HANDLE.with(|slot| *slot.borrow_mut() = Some(handle));

        let data: IDataObject = ShelfData.into();
        let source: IDropSource = ShelfSource.into();
        let mut effect = DROPEFFECT_COPY | DROPEFFECT_MOVE;
        // SAFETY: Both COM objects are live local variables and `effect` is a valid out-param.
        // This blocks the UI thread until the user drops or cancels, which is inherent to
        // how Windows implements drag-and-drop.
        let result = unsafe {
            DoDragDrop(
                &data,
                &source,
                DROPEFFECT_COPY | DROPEFFECT_MOVE,
                &mut effect,
            )
        };

        if let Some(handle) = DROP_HANDLE.with(|slot| slot.borrow_mut().take()) {
            // SAFETY: The handle came from `GlobalAlloc` above and is no longer referenced.
            unsafe {
                let _ = GlobalFree(Some(handle));
            }
        }

        result == DRAGDROP_S_DROP
    }
}

#[cfg(not(windows))]
mod platform {
    use std::path::PathBuf;

    /// Dragging out to the OS is not wired up outside Windows yet.
    pub fn drag_out(_paths: &[PathBuf]) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str, bytes: usize) -> PathBuf {
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, vec![b'x'; bytes]).unwrap();
        path
    }

    #[test]
    fn add_dedupes_and_dismiss_removes_by_index() {
        let path = temp("shelf_add_test.txt", 2048);
        let mut shelf = Shelf::new();

        assert!(shelf.add(path.clone()).is_some());
        assert!(shelf.add(path.clone()).is_none(), "duplicate must be ignored");
        assert_eq!(shelf.len(), 1);

        shelf.dismiss(0);
        assert!(shelf.is_empty());

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn directories_are_rejected_with_an_error() {
        let mut shelf = Shelf::new();
        assert!(shelf.add(std::env::temp_dir()).is_none());
        assert!(shelf.error().is_some());
        assert!(shelf.is_empty());
    }

    #[test]
    fn prune_drops_entries_whose_file_is_gone() {
        let path = temp("shelf_prune_test.txt", 8);
        let mut shelf = Shelf::new();
        shelf.add(path.clone()).unwrap();

        std::fs::remove_file(&path).unwrap();
        shelf.prune();
        assert!(shelf.is_empty());
    }

    #[test]
    fn human_size_is_readable() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(2048), "2.0 KB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0 MB");
    }
}
