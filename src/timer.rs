use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerState {
    Idle,
    Running,
    Paused,
    Finished,
}

pub struct Timer {
    pub label: String,
    state: TimerState,
    duration: Duration,
    remaining: Duration,
    last: Instant,
}

impl Timer {
    pub fn new() -> Self {
        let duration = Duration::from_secs(5 * 60);
        Self {
            label: "Focus".into(),
            state: TimerState::Idle,
            duration,
            remaining: duration,
            last: Instant::now(),
        }
    }

    pub fn state(&self) -> TimerState {
        self.state
    }

    pub fn duration(&self) -> Duration {
        self.duration
    }

    pub fn progress(&self) -> f32 {
        if self.duration.is_zero() {
            return 0.0;
        }
        1.0 - (self.remaining.as_secs_f32() / self.duration.as_secs_f32()).clamp(0.0, 1.0)
    }

    pub fn set_duration(&mut self, duration: Duration) {
        self.duration = duration.max(Duration::from_secs(1));
        self.reset();
    }

    pub fn reset(&mut self) {
        self.state = TimerState::Idle;
        self.remaining = self.duration;
    }

    pub fn toggle(&mut self) {
        match self.state {
            TimerState::Idle | TimerState::Paused => {
                self.last = Instant::now();
                self.state = TimerState::Running;
            }
            TimerState::Running => self.state = TimerState::Paused,
            TimerState::Finished => self.reset(),
        }
    }

    /// Advances the countdown. Returns `true` on the tick it reaches zero.
    pub fn tick(&mut self) -> bool {
        if self.state != TimerState::Running {
            return false;
        }
        let now = Instant::now();
        let elapsed = now.duration_since(self.last);
        self.last = now;
        self.remaining = self.remaining.saturating_sub(elapsed);
        if self.remaining.is_zero() {
            self.state = TimerState::Finished;
            return true;
        }
        false
    }

    pub fn label_time(&self) -> String {
        let secs = self.remaining.as_secs();
        format!("{:02}:{:02}", secs / 60, secs % 60)
    }
}
