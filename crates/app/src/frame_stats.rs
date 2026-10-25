//! How often the desk draws and how long a frame takes on the CPU, summed up for the detailed
//! log: the measure for "the app costs too much" before and after a change.

/// Seconds a summary covers.
pub const SUMMARY_SECONDS: f64 = 10.0;

const MILLISECONDS_PER_SECOND: f64 = 1000.0;

/// What the frames of one summary period cost.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameSummary {
    pub frames_per_second: f64,
    pub average_milliseconds: f64,
    pub longest_milliseconds: f64,
}

/// Counts the frames since the period began.
#[derive(Debug, Default)]
pub struct FrameStats {
    since: Option<f64>,
    frames: u32,
    cpu_seconds: f64,
    longest_seconds: f64,
}

impl FrameStats {
    /// One frame at `now` that took `cpu_seconds` (none on the very first). A summary comes back
    /// once `SUMMARY_SECONDS` are over, and the next period begins.
    pub fn record(&mut self, now: f64, cpu_seconds: Option<f32>) -> Option<FrameSummary> {
        let since = *self.since.get_or_insert(now);
        self.frames += 1;
        let cpu_seconds = f64::from(cpu_seconds.unwrap_or_default());
        self.cpu_seconds += cpu_seconds;
        self.longest_seconds = self.longest_seconds.max(cpu_seconds);
        let elapsed = now - since;
        if elapsed < SUMMARY_SECONDS {
            return None;
        }
        let summary = FrameSummary {
            frames_per_second: f64::from(self.frames) / elapsed,
            average_milliseconds: self.cpu_seconds / f64::from(self.frames)
                * MILLISECONDS_PER_SECOND,
            longest_milliseconds: self.longest_seconds * MILLISECONDS_PER_SECOND,
        };
        *self = Self {
            since: Some(now),
            ..Self::default()
        };
        Some(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_summary_comes_once_the_period_is_over_and_counts_its_frames() {
        let mut stats = FrameStats::default();
        assert_eq!(stats.record(0.0, None), None);
        assert_eq!(stats.record(5.0, Some(0.002)), None);
        let summary = stats.record(SUMMARY_SECONDS, Some(0.004)).unwrap();
        assert_eq!(summary.frames_per_second, 0.3);
        assert!((summary.average_milliseconds - 2.0).abs() < 1e-6);
        assert!((summary.longest_milliseconds - 4.0).abs() < 1e-6);
    }

    #[test]
    fn the_next_period_starts_from_nothing() {
        let mut stats = FrameStats::default();
        stats.record(0.0, Some(0.5));
        stats.record(SUMMARY_SECONDS, Some(0.5));
        assert_eq!(stats.record(SUMMARY_SECONDS + 1.0, Some(0.001)), None);
        let summary = stats.record(2.0 * SUMMARY_SECONDS, Some(0.001)).unwrap();
        assert_eq!(summary.frames_per_second, 0.2);
        assert!((summary.longest_milliseconds - 1.0).abs() < 1e-6);
    }
}
