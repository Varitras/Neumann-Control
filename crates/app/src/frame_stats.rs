//! How often the desk draws and how long a frame takes on the CPU, summed up for the detailed
//! log: the measure for "the app costs too much" before and after a change, with what asked for
//! the frames most often.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

use eframe::egui::RepaintCause;

/// Seconds a summary covers.
pub const SUMMARY_SECONDS: f64 = 10.0;

const MILLISECONDS_PER_SECOND: f64 = 1000.0;
/// How many of the causes a summary names.
pub const NAMED_CAUSES: usize = 3;
/// A cause's file is named from the folder before its crate's `src` on.
const SOURCE_FOLDER: &str = "src";

/// What the frames of one summary period cost.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameSummary {
    pub frames_per_second: f64,
    pub average_milliseconds: f64,
    pub longest_milliseconds: f64,
    /// The causes that asked for the most frames, with in how many frames, most first.
    pub causes: Vec<(String, u32)>,
}

/// Counts the frames since the period began.
#[derive(Debug, Default)]
pub struct FrameStats {
    since: Option<f64>,
    frames: u32,
    cpu_seconds: f64,
    longest_seconds: f64,
    causes: BTreeMap<String, u32>,
}

impl FrameStats {
    /// One frame at `now` that took `cpu_seconds` (none on the very first), asked for by
    /// `causes`. A summary comes back once `SUMMARY_SECONDS` are over, and the next period begins.
    pub fn record(
        &mut self,
        now: f64,
        cpu_seconds: Option<f32>,
        causes: &[String],
    ) -> Option<FrameSummary> {
        let since = *self.since.get_or_insert(now);
        self.frames += 1;
        // A cause asks many times in one frame (a widget per speaker); it counts once.
        let distinct: BTreeSet<&String> = causes.iter().collect();
        for cause in distinct {
            *self.causes.entry(cause.clone()).or_default() += 1;
        }
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
            causes: most_frequent(std::mem::take(&mut self.causes)),
        };
        *self = Self {
            since: Some(now),
            ..Self::default()
        };
        Some(summary)
    }
}

/// The `NAMED_CAUSES` most frequent causes, most first; equal counts by name.
fn most_frequent(causes: BTreeMap<String, u32>) -> Vec<(String, u32)> {
    let mut causes: Vec<(String, u32)> = causes.into_iter().collect();
    causes.sort_by_key(|(_, frames)| std::cmp::Reverse(*frames));
    causes.truncate(NAMED_CAUSES);
    causes
}

/// A cause as the log names it: its file from the crate folder on (`egui-0.37.0/src/...`), so no
/// user folder of the build machine reaches the log, and its line.
pub fn cause_name(cause: &RepaintCause) -> String {
    let parts: Vec<&str> = Path::new(cause.file)
        .components()
        .filter_map(|part| match part {
            Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .collect();
    let source = parts.iter().rposition(|part| *part == SOURCE_FOLDER);
    let from = source.map_or(parts.len().saturating_sub(1), |at| at.saturating_sub(1));
    let file = parts[from..].join("/");
    if cause.reason.is_empty() {
        return format!("{file}:{}", cause.line);
    }
    format!("{file}:{} {}", cause.line, cause.reason)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cause(file: &'static str, line: u32) -> RepaintCause {
        RepaintCause {
            file,
            line,
            reason: "".into(),
        }
    }

    #[test]
    fn a_summary_names_the_causes_that_asked_most() {
        let mut stats = FrameStats::default();
        let spinner = "egui/src/spinner.rs:40".to_owned();
        let meters = "app/src/meters.rs:142".to_owned();
        stats.record(0.0, None, std::slice::from_ref(&meters));
        stats.record(
            1.0,
            None,
            &[spinner.clone(), meters.clone(), spinner.clone()],
        );
        stats.record(2.0, None, std::slice::from_ref(&spinner));
        let summary = stats
            .record(SUMMARY_SECONDS, None, std::slice::from_ref(&spinner))
            .unwrap();
        assert_eq!(summary.causes, [(spinner, 3), (meters, 2)]);
        let next = stats.record(2.0 * SUMMARY_SECONDS, None, &[]).unwrap();
        assert!(next.causes.is_empty(), "a new period counts anew");
    }

    #[test]
    fn a_summary_names_at_most_a_few_causes() {
        let mut stats = FrameStats::default();
        let many: Vec<String> = (0..NAMED_CAUSES + 2)
            .map(|n| format!("file.rs:{n}"))
            .collect();
        stats.record(0.0, None, &many);
        let summary = stats.record(SUMMARY_SECONDS, None, &many).unwrap();
        assert_eq!(summary.causes.len(), NAMED_CAUSES);
    }

    /// The build machine's user folder (in the path of every crate from the registry) never
    /// reaches the log.
    #[test]
    fn a_cause_is_named_from_its_crate_on() {
        let registry = cause(
            r"C:\Users\someone\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\egui-0.37.0\src\widgets\spinner.rs",
            40,
        );
        assert_eq!(
            cause_name(&registry),
            "egui-0.37.0/src/widgets/spinner.rs:40"
        );
        let own = cause(r"crates\app\src\views\meters.rs", 142);
        assert_eq!(cause_name(&own), "app/src/views/meters.rs:142");
    }

    #[test]
    fn a_summary_comes_once_the_period_is_over_and_counts_its_frames() {
        let mut stats = FrameStats::default();
        assert_eq!(stats.record(0.0, None, &[]), None);
        assert_eq!(stats.record(5.0, Some(0.002), &[]), None);
        let summary = stats.record(SUMMARY_SECONDS, Some(0.004), &[]).unwrap();
        assert_eq!(summary.frames_per_second, 0.3);
        assert!((summary.average_milliseconds - 2.0).abs() < 1e-6);
        assert!((summary.longest_milliseconds - 4.0).abs() < 1e-6);
    }

    #[test]
    fn the_next_period_starts_from_nothing() {
        let mut stats = FrameStats::default();
        stats.record(0.0, Some(0.5), &[]);
        stats.record(SUMMARY_SECONDS, Some(0.5), &[]);
        assert_eq!(stats.record(SUMMARY_SECONDS + 1.0, Some(0.001), &[]), None);
        let summary = stats
            .record(2.0 * SUMMARY_SECONDS, Some(0.001), &[])
            .unwrap();
        assert_eq!(summary.frames_per_second, 0.2);
        assert!((summary.longest_milliseconds - 1.0).abs() < 1e-6);
    }
}
