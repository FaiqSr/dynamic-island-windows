use std::path::{Path, PathBuf};
use std::time::Duration;

use rodio::source::TrackPosition;
use rodio::{Decoder, Source};

/// Headroom for the local decoder. Output level is the *system* volume, so the decoder itself
/// stays at a fixed gain instead of carrying a second, competing volume.
pub const VOLUME: f32 = 0.8;

type Decoded = TrackPosition<rodio::source::Amplify<Decoder<std::io::BufReader<std::fs::File>>>>;

pub struct Track {
    pub title: String,
    pub path: PathBuf,
    pub duration: Option<Duration>,
}

pub struct Audio {
    engine: Option<Engine>,
    track: Option<Track>,
    error: Option<String>,
}

struct Engine {
    _sink: rodio::MixerDeviceSink,
    player: rodio::Player,
}

impl Audio {
    pub fn new() -> Self {
        // Dropping the device sink stops audio, so it lives as long as the app.
        let engine = rodio::DeviceSinkBuilder::open_default_sink()
            .map(|sink| {
                let player = rodio::Player::connect_new(sink.mixer());
                Engine {
                    _sink: sink,
                    player,
                }
            })
            .ok();
        Self {
            error: engine
                .is_none()
                .then(|| "No audio output device found".into()),
            engine,
            track: None,
        }
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn track(&self) -> Option<&Track> {
        self.track.as_ref()
    }

    pub fn load(&mut self, path: PathBuf) {
        let Some(engine) = self.engine.as_ref() else {
            return;
        };
        let title = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Unknown track".into());
        match open_source(&path) {
            Ok(source) => {
                let duration = source.total_duration();
                engine.player.clear();
                engine.player.append(source);
                engine.player.play();
                self.error = None;
                self.track = Some(Track {
                    title,
                    path,
                    duration,
                });
            }
            Err(e) => self.error = Some(format!("Cannot play \"{}\": {e}", path.display())),
        }
    }

    pub fn toggle(&mut self) {
        let Some(engine) = self.engine.as_ref() else {
            return;
        };
        if self.track.is_none() {
            return;
        }
        if engine.player.is_paused() {
            engine.player.play();
        } else {
            engine.player.pause();
        }
    }

    pub fn is_playing(&self) -> bool {
        self.engine
            .as_ref()
            .is_some_and(|e| self.track.is_some() && !e.player.is_paused() && !e.player.empty())
    }

    /// Loops the current track when the mixer runs dry.
    pub fn tick(&mut self) {
        let Some(path) = self.track.as_ref().map(|t| t.path.clone()) else {
            return;
        };
        if self.is_playing() && self.engine.as_ref().is_some_and(|e| e.player.empty()) {
            self.load(path);
        }
    }

    pub fn position(&self) -> Duration {
        self.engine
            .as_ref()
            .map_or(Duration::ZERO, |e| e.player.get_pos())
    }

    pub fn seek_ratio(&mut self, ratio: f32) {
        let Some(engine) = self.engine.as_ref() else {
            return;
        };
        let Some(duration) = self.track.as_ref().and_then(|t| t.duration) else {
            return;
        };
        let _ = engine
            .player
            .try_seek(duration.mul_f32(ratio.clamp(0.0, 1.0)));
    }
}

fn open_source(path: &Path) -> Result<Decoded, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let decoder = Decoder::try_from(file).map_err(|e| e.to_string())?;
    Ok(decoder.amplify(VOLUME).track_position())
}
