//! System output volume control.
//!
//! The music page slider drives the Windows output endpoint (Core Audio
//! `IAudioEndpointVolume`), so it changes the real system volume — the same fader the hardware
//! keys and the Windows mixer move — rather than a private gain inside the island. Mirroring
//! the structure of `src/media.rs`, the non-Windows module is a stub.

#[cfg(windows)]
pub use platform::SystemVolume;

/// Cached master volume of the default playback device, plus the mute flag.
///
/// The device itself is the source of truth and `SystemVolume` is the only writer; this copy
/// exists because the mute state needs a level to restore, which Windows does not expose here.
/// `muted` is `None` when not muted, otherwise `Some(level to restore)`.
pub struct VolumeState {
    level: f32,
    muted: Option<f32>,
}

impl VolumeState {
    pub fn new() -> Self {
        Self {
            level: 0.8,
            muted: None,
        }
    }

    /// Volume the output is at, which is zero while muted.
    pub fn level(&self) -> f32 {
        self.muted.unwrap_or(self.level)
    }

    pub fn is_muted(&self) -> bool {
        self.muted.is_some()
    }

    /// Pulls the level from the OS. Ignored while muted from the island, so a poll cannot
    /// immediately undo that.
    pub fn sync(&mut self, level: Option<f32>) {
        if self.muted.is_none()
            && let Some(level) = level
        {
            self.level = level.clamp(0.0, 1.0);
        }
    }

    /// Sets the level from a slider drag; dragging also unmutes, like the Windows mixer.
    pub fn set(&mut self, level: f32) {
        self.level = level.clamp(0.0, 1.0);
        self.muted = None;
    }

    /// Mutes at the current level and returns the level to restore later, or unmutes and
    /// returns the level that is being played again.
    pub fn toggle_mute(&mut self) -> f32 {
        self.muted = match self.muted {
            Some(level) => {
                self.level = level;
                None
            }
            None => Some(self.level),
        };
        self.level()
    }
}

#[cfg(windows)]
mod platform {
    use windows::Win32::Media::Audio::{
        IMMDeviceEnumerator, MMDeviceEnumerator, eMultimedia, eRender,
    };
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

    pub struct SystemVolume {
        endpoint: IAudioEndpointVolume,
    }

    impl SystemVolume {
        /// `None` when the audio service is unavailable, which keeps the slider hidden
        /// instead of showing one that does nothing.
        pub fn new() -> Option<Self> {
            // The endpoint object is created once and then reused, so the COM apartment must
            // outlive it. It is only balanced when this call is the one that initialised COM;
            // otherwise the apartment may belong to someone else and is left alone.
            let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
            let created = unsafe {
                CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .ok()
                    .and_then(|enumerator| {
                        enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia).ok()
                    })
                    .and_then(|device| device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok())
            };
            match created {
                Some(endpoint) => Some(Self { endpoint }),
                None => {
                    if initialized {
                        unsafe { CoUninitialize() };
                    }
                    None
                }
            }
        }

        /// Current master volume in `0.0..=1.0`.
        pub fn level(&self) -> f32 {
            unsafe { self.endpoint.GetMasterVolumeLevelScalar() }.unwrap_or(0.0)
        }

        /// Best-effort like every other WinRT/Core Audio call in the app.
        pub fn set_level(&self, level: f32) {
            let _ = unsafe {
                self.endpoint
                    .SetMasterVolumeLevelScalar(level.clamp(0.0, 1.0), std::ptr::null())
            };
        }
    }
}

#[cfg(not(windows))]
mod platform {
    /// Non-Windows builds have no system volume endpoint yet.
    pub struct SystemVolume;

    impl SystemVolume {
        pub fn new() -> Option<Self> {
            None
        }

        pub fn level(&self) -> f32 {
            0.0
        }

        pub fn set_level(&self, _level: f32) {}
    }
}

#[cfg(not(windows))]
pub use platform::SystemVolume;
