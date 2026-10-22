use std::time::{Duration, Instant};

/// Exponential smoothing toward a target, driven by real elapsed wall-clock
/// time so motion speed is independent of the frame rate.
#[derive(Debug, Clone, Copy)]
pub struct Spring {
    value: f32,
    last: Instant,
}

impl Spring {
    pub fn new(value: f32) -> Self {
        Self {
            value,
            last: Instant::now(),
        }
    }

    /// Returns the eased `t` in `0..=1` for a two-state expand/collapse.
    pub fn animate_to(&mut self, target: f32, speed: f32) -> f32 {
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f32().min(0.1);
        self.last = now;
        let factor = 1.0 - (-dt * speed).exp();
        self.value += (target - self.value) * factor;
        if (target - self.value).abs() < 0.001 {
            self.value = target;
        }
        let t = self.value.clamp(0.0, 1.0);
        // Smoothstep keeps the corners of the island from overshooting.
        t * t * (3.0 - 2.0 * t)
    }

    pub fn is_settled(&self, target: f32) -> bool {
        self.value == target
    }
}

/// Progress of a page-to-page transition, run on wall-clock time so the duration is
/// independent of the frame rate.
///
/// A transition remembers which way the user moved: forward slides the new page in from
/// the right, backward from the left, so the motion matches the arrow that was pressed.
#[derive(Debug, Clone, Copy)]
pub struct PageTransition {
    /// Signed page index we are leaving (only its sign relative to `to` matters).
    from: usize,
    to: usize,
    direction: f32,
    started: Instant,
    running: bool,
}

/// How long a page transition takes. Short enough to feel immediate, long enough to read.
pub const PAGE_TRANSITION: Duration = Duration::from_millis(260);

impl PageTransition {
    pub fn new(tab: usize) -> Self {
        Self {
            from: tab,
            to: tab,
            direction: 1.0,
            started: Instant::now(),
            running: false,
        }
    }

    /// Starts a transition towards `to`, or does nothing if it is already there.
    pub fn start(&mut self, from: usize, to: usize) {
        self.start_with_direction(from, to, if to >= from { 1.0 } else { -1.0 });
    }

    pub fn start_with_direction(&mut self, from: usize, to: usize, direction: f32) {
        if from == to {
            return;
        }
        self.from = from;
        self.to = to;
        self.direction = direction.signum();
        self.started = Instant::now();
        self.running = true;
    }

    /// Raw progress in `0..=1`; `1.0` once the transition has finished.
    pub fn progress(&self) -> f32 {
        if !self.running {
            return 1.0;
        }
        (self.started.elapsed().as_secs_f32() / PAGE_TRANSITION.as_secs_f32()).clamp(0.0, 1.0)
    }

    pub fn is_running(&self, now: Instant) -> bool {
        self.running && now.duration_since(self.started) < PAGE_TRANSITION
    }

    /// The page being left while a transition runs.
    pub fn outgoing(&self) -> usize {
        self.from
    }

    pub fn incoming(&self) -> usize {
        self.to
    }

    /// `+1` when moving to a higher page index, `-1` when moving to a lower one.
    pub fn direction(&self) -> f32 {
        self.direction
    }

    pub fn finish(&mut self) {
        self.running = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_transition_is_already_settled() {
        let transition = PageTransition::new(2);
        assert_eq!(transition.progress(), 1.0);
        assert!(!transition.is_running(Instant::now()));
    }

    #[test]
    fn direction_follows_the_page_delta() {
        let mut forward = PageTransition::new(0);
        forward.start(0, 2);
        assert_eq!(forward.direction(), 1.0);
        assert_eq!(forward.outgoing(), 0);
        assert_eq!(forward.incoming(), 2);

        let mut backward = PageTransition::new(0);
        backward.start(3, 1);
        assert_eq!(backward.direction(), -1.0);
    }

    #[test]
    fn switching_to_the_same_page_does_not_start() {
        let mut transition = PageTransition::new(1);
        transition.start(1, 1);
        assert_eq!(transition.progress(), 1.0);
        assert!(!transition.is_running(Instant::now()));
    }

    #[test]
    fn progress_is_clamped_and_completes() {
        let mut transition = PageTransition::new(0);
        transition.start(0, 1);
        // Just after starting the progress must still be a fraction, never > 1.
        let p = transition.progress();
        assert!((0.0..=1.0).contains(&p));
        transition.finish();
        assert_eq!(transition.progress(), 1.0);
    }
}
