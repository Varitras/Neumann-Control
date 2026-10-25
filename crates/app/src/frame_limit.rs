//! Keeps frames at least a sixtieth of a second apart. The app presents without vsync (Mailbox,
//! for remote desktops), so every egui animation that asks for the next frame at once (a scroll
//! bar fading in, a spinner) drew a frame every millisecond; measured at startup: one to two
//! hundred frames a second and 40 % of a core.

use std::time::{Duration, Instant};

/// Sixty frames a second, the rate vsync would allow on a common monitor.
pub const MIN_FRAME_INTERVAL: Duration = Duration::from_micros(16_667);

#[derive(Debug, Default)]
pub struct FrameLimit {
    last: Option<Instant>,
}

impl FrameLimit {
    /// How long a frame starting `now` waits so it is `MIN_FRAME_INTERVAL` after the one before.
    pub fn pause(&mut self, now: Instant) -> Duration {
        let pause = self.last.map_or(Duration::ZERO, |last| {
            MIN_FRAME_INTERVAL.saturating_sub(now.saturating_duration_since(last))
        });
        self.last = Some(now + pause);
        pause
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_right_after_another_waits_for_the_rest_of_the_interval() {
        let mut limit = FrameLimit::default();
        let start = Instant::now();
        assert_eq!(limit.pause(start), Duration::ZERO, "the first frame");
        let soon = start + Duration::from_millis(1);
        assert_eq!(
            limit.pause(soon),
            MIN_FRAME_INTERVAL - Duration::from_millis(1)
        );
        // The waited frame counts from when it ran, not from when it asked.
        let next = start + MIN_FRAME_INTERVAL + Duration::from_millis(2);
        assert_eq!(
            limit.pause(next),
            MIN_FRAME_INTERVAL - Duration::from_millis(2)
        );
    }

    #[test]
    fn a_frame_after_a_quiet_spell_does_not_wait() {
        let mut limit = FrameLimit::default();
        let start = Instant::now();
        limit.pause(start);
        assert_eq!(limit.pause(start + Duration::from_secs(1)), Duration::ZERO);
    }
}
