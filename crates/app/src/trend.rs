//! A value's course over the last two hours of this session: only its change points, so a value
//! that rarely changes costs one point, and gaps where the speaker could not be read.

use std::collections::VecDeque;

/// How far back the device panel shows a trend.
pub const TREND_WINDOW_SECONDS: f64 = 7200.0;
/// A value that changes at every poll for hours still keeps a bounded list.
pub const TREND_POINT_LIMIT: usize = 4096;
/// The shortest span a trend is drawn over, so the first readings are not a line at the edge.
pub const TREND_MIN_SPAN_SECONDS: f64 = 60.0;

const SECONDS_PER_MINUTE: f64 = 60.0;

/// Seconds as whole minutes, as a trend reads them out; never negative.
pub fn whole_minutes(seconds: f64) -> u64 {
    (seconds / SECONDS_PER_MINUTE).round().max(0.0) as u64
}

/// The seconds a trend is drawn over: from the first reading, at least
/// `TREND_MIN_SPAN_SECONDS`, at most the window.
pub fn shown_span(first: Option<f64>, now: f64) -> f64 {
    let since_first = first.map_or(0.0, |first| now - first);
    since_first.clamp(TREND_MIN_SPAN_SECONDS, TREND_WINDOW_SECONDS)
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrendPoint<T> {
    /// Seconds of the UI clock.
    pub time: f64,
    /// `None` from here on the speaker could not be read.
    pub value: Option<T>,
}

#[derive(Clone, Debug)]
pub struct Trend<T> {
    points: VecDeque<TrendPoint<T>>,
}

impl<T> Default for Trend<T> {
    fn default() -> Self {
        Trend {
            points: VecDeque::new(),
        }
    }
}

impl<T: PartialEq> Trend<T> {
    /// Notes what was read at `now`; only a change adds a point.
    pub fn record(&mut self, now: f64, value: Option<T>) {
        let unchanged = self.points.back().is_some_and(|last| last.value == value);
        let nothing_yet = self.points.is_empty() && value.is_none();
        if !unchanged && !nothing_yet {
            self.points.push_back(TrendPoint { time: now, value });
        }
        self.prune(now);
    }

    /// Drops what lies before the window, except the last point before it: that one holds the
    /// value at the window's start.
    fn prune(&mut self, now: f64) {
        let start = now - TREND_WINDOW_SECONDS;
        while self.points.len() > 1 && self.points[1].time <= start {
            self.points.pop_front();
        }
        while self.points.len() > TREND_POINT_LIMIT {
            self.points.pop_front();
        }
    }

    pub fn points(&self) -> impl Iterator<Item = &TrendPoint<T>> {
        self.points.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    pub fn first_time(&self) -> Option<f64> {
        self.points.front().map(|point| point.time)
    }

    /// The value at `time`: that of the last point at or before it.
    pub fn value_at(&self, time: f64) -> Option<&T> {
        self.points
            .iter()
            .rev()
            .find(|point| point.time <= time)
            .and_then(|point| point.value.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn times<T: PartialEq>(trend: &Trend<T>) -> Vec<f64> {
        trend.points().map(|point| point.time).collect()
    }

    #[test]
    fn records_only_changes() {
        let mut trend = Trend::default();
        trend.record(0.0, Some(310.0));
        trend.record(1.0, Some(310.0));
        trend.record(2.0, Some(311.0));
        assert_eq!(times(&trend), vec![0.0, 2.0]);
    }

    #[test]
    fn a_gap_is_a_none_point() {
        let mut trend = Trend::default();
        trend.record(0.0, Some(310.0));
        trend.record(5.0, None);
        trend.record(9.0, Some(310.0));
        assert_eq!(times(&trend), vec![0.0, 5.0, 9.0]);
        assert_eq!(trend.value_at(6.0), None);
        assert_eq!(trend.value_at(9.5), Some(&310.0));
    }

    #[test]
    fn nothing_read_yet_is_no_gap() {
        let mut trend: Trend<f64> = Trend::default();
        trend.record(0.0, None);
        assert!(trend.is_empty());
    }

    #[test]
    fn drops_points_older_than_the_window_but_keeps_the_start() {
        let mut trend = Trend::default();
        trend.record(0.0, Some(1.0));
        trend.record(100.0, Some(2.0));
        trend.record(200.0, Some(3.0));
        let now = 150.0 + TREND_WINDOW_SECONDS;
        trend.record(now, Some(4.0));
        // The point at 100 holds the value at the window's start (150), so it stays.
        assert_eq!(times(&trend), vec![100.0, 200.0, now]);
        assert_eq!(trend.value_at(150.0), Some(&2.0));
    }

    #[test]
    fn an_unchanged_value_still_moves_the_window() {
        let mut trend = Trend::default();
        trend.record(0.0, Some(1.0));
        trend.record(10.0, Some(2.0));
        trend.record(20.0 + TREND_WINDOW_SECONDS, Some(2.0));
        assert_eq!(times(&trend), vec![10.0]);
    }

    #[test]
    fn is_capped() {
        let mut trend = Trend::default();
        for step in 0..TREND_POINT_LIMIT + 10 {
            trend.record(step as f64 * 0.1, Some(step));
        }
        assert_eq!(trend.points().count(), TREND_POINT_LIMIT);
        assert_eq!(
            trend.points().next().map(|point| point.value),
            Some(Some(10))
        );
    }

    #[test]
    fn the_span_runs_from_the_first_reading_within_its_bounds() {
        assert_eq!(shown_span(Some(100.0), 400.0), 300.0);
        assert_eq!(shown_span(Some(100.0), 110.0), TREND_MIN_SPAN_SECONDS);
        assert_eq!(shown_span(None, 110.0), TREND_MIN_SPAN_SECONDS);
        let late = 100.0 + 3.0 * TREND_WINDOW_SECONDS;
        assert_eq!(shown_span(Some(100.0), late), TREND_WINDOW_SECONDS);
    }

    #[test]
    fn whole_minutes_round_and_stay_positive() {
        assert_eq!(whole_minutes(89.0), 1);
        assert_eq!(whole_minutes(91.0), 2);
        assert_eq!(whole_minutes(-5.0), 0);
    }

    #[test]
    fn value_at_before_the_first_point_is_unknown() {
        let mut trend = Trend::default();
        trend.record(5.0, Some(1.0));
        assert_eq!(trend.value_at(4.0), None);
        assert_eq!(trend.value_at(5.0), Some(&1.0));
    }
}
