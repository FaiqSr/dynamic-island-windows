//! Windows system media session (SMTC) bridge.
//!
//! Apps such as Spotify, browsers and the Windows media player publish their
//! playback state to a system media session. Reading that session lets the
//! island mirror whatever is playing outside of this app.

#[cfg(windows)]
pub use platform::SystemMedia;

/// Playback facts the island needs. Everything is owned so the UI can keep a copy.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub title: String,
    pub artist: String,
    pub app: String,
    pub playing: bool,
    pub position: std::time::Duration,
    pub duration: Option<std::time::Duration>,
}

#[cfg(windows)]
mod platform {
    use super::Snapshot;
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSession as Session,
        GlobalSystemMediaTransportControlsSessionManager as SessionManager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
    };

    pub struct SystemMedia {
        manager: SessionManager,
    }

    impl SystemMedia {
        /// `None` when the OS has no media control, which is common on older builds.
        pub fn new() -> Option<Self> {
            // WinRT resolves the apartment on first use; ignoring failures below keeps the app
            // usable when media control is unavailable.
            SessionManager::RequestAsync()
                .ok()?
                .get()
                .ok()
                .map(|manager| Self { manager })
        }

        /// Current session for the front app, falling back to any session that is playing.
        fn session(&self) -> Option<Session> {
            if let Ok(session) = self.manager.GetCurrentSession() {
                return Some(session);
            }
            let sessions = self.manager.GetSessions().ok()?;
            let mut fallback: Option<Session> = None;
            for index in 0..sessions.Size().ok()? {
                let Ok(session) = sessions.GetAt(index) else {
                    continue;
                };
                match playback_status(&session) {
                    Status::Playing => return Some(session),
                    Status::Closed => {}
                    _ => {
                        fallback.get_or_insert(session);
                    }
                }
            }
            fallback
        }

        pub fn snapshot(&self) -> Option<Snapshot> {
            let session = self.session()?;
            let props = session.TryGetMediaPropertiesAsync().ok()?.get().ok()?;
            let title = props
                .Title()
                .map(|t| t.to_string_lossy())
                .unwrap_or_default();
            let artist = props
                .Artist()
                .map(|a| a.to_string_lossy())
                .unwrap_or_default();
            if title.trim().is_empty() && artist.trim().is_empty() {
                return None;
            }
            let app = session
                .SourceAppUserModelId()
                .map(|id| app_name(&id.to_string_lossy()))
                .unwrap_or_else(|_| "System".into());

            let mut position = std::time::Duration::ZERO;
            let mut duration = None;
            if let Ok(timeline) = session.GetTimelineProperties() {
                if let Ok(pos) = timeline.Position() {
                    position = ticks(pos.Duration);
                }
                if let Ok(end) = timeline.EndTime() {
                    let end = ticks(end.Duration);
                    if !end.is_zero() {
                        duration = Some(end);
                    }
                }
            }

            Some(Snapshot {
                title,
                artist,
                app,
                playing: playback_status(&session) == Status::Playing,
                position,
                duration,
            })
        }

        pub fn play_pause(&self) -> bool {
            self.session()
                .is_some_and(|s| s.TryTogglePlayPauseAsync().is_ok_and(|op| op.get().is_ok()))
        }

        /// Seeks by a signed offset in 100 ns ticks, clamped to the track length.
        pub fn seek_by(&self, delta_ticks: i64) -> bool {
            let Some(session) = self.session() else {
                return false;
            };
            let Ok(timeline) = session.GetTimelineProperties() else {
                return false;
            };
            let current = timeline.Position().map(|p| p.Duration).unwrap_or(0);
            let end = timeline.EndTime().map(|p| p.Duration).unwrap_or(i64::MAX);
            let target = current.saturating_add(delta_ticks).clamp(0, end);
            session
                .TryChangePlaybackPositionAsync(target)
                .is_ok_and(|op| op.get().is_ok())
        }
    }

    fn ticks(value: i64) -> std::time::Duration {
        std::time::Duration::from_nanos(value.max(0) as u64 * 100)
    }

    fn playback_status(session: &Session) -> Status {
        session
            .GetPlaybackInfo()
            .and_then(|info| info.PlaybackStatus())
            .unwrap_or(Status::Closed)
    }

    /// Turns `Spotify.exe` / `chrome.exe` style ids into a readable app name.
    fn app_name(id: &str) -> String {
        let trimmed = id.rsplit('!').next().unwrap_or(id);
        let stem = trimmed
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(trimmed)
            .split('.')
            .next()
            .unwrap_or(trimmed);
        let mut chars = stem.chars();
        match chars.next() {
            Some(first) if !stem.is_empty() => first.to_uppercase().chain(chars).collect(),
            _ => "System".into(),
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use super::Snapshot;

    /// Non-Windows builds have no system media session yet.
    pub struct SystemMedia;

    impl SystemMedia {
        pub fn new() -> Option<Self> {
            None
        }

        pub fn snapshot(&self) -> Option<Snapshot> {
            None
        }

        pub fn play_pause(&self) -> bool {
            false
        }

        pub fn seek_by(&self, _delta_ticks: i64) -> bool {
            false
        }
    }
}

#[cfg(not(windows))]
pub use platform::SystemMedia;
