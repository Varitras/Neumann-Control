//! The level fader's wheel: steps collect until the wheel goes quiet, then one write goes out.
//! Times are seconds of the UI clock.

use std::ops::RangeInclusive;

use neumann_control_core::catalog::Catalog;

use crate::paths;

pub const WHEEL_STEP_DB: f64 = 0.5;
pub const WHEEL_IDLE_SECONDS: f64 = 0.3;
/// Fractions of a notch are counted in whole units so ten tenths make exactly one notch.
const NOTCH_RESOLUTION: f64 = 1000.0;

/// The catalog range of the output level, which the fader drives.
pub fn output_level_range() -> RangeInclusive<f64> {
    let path = paths::path(paths::OUTPUT_LEVEL);
    let entry =
        paths::catalog_entry(Catalog::embedded(), &path).expect("catalog has the output level");
    let min = entry.min.expect("output level has a minimum");
    let max = entry.max.expect("output level has a maximum");
    min..=max
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FaderState {
    pending: Option<f64>,
    last_step: f64,
    /// Wheel movement short of a whole notch, in 1/NOTCH_RESOLUTION notches.
    partial_notch: i64,
}

impl FaderState {
    /// `notches` may be fractional (touchpads, high-resolution wheels); one step of
    /// WHEEL_STEP_DB per whole notch, the rest is kept for the next event. `range` is the
    /// allowed value range of the faded entry.
    pub fn on_wheel(&mut self, notches: f64, current: f64, range: RangeInclusive<f64>, now: f64) {
        self.partial_notch += (notches * NOTCH_RESOLUTION).round() as i64;
        let resolution = NOTCH_RESOLUTION as i64;
        let whole = self.partial_notch / resolution;
        self.partial_notch %= resolution;
        self.last_step = now;
        if whole == 0 {
            return;
        }
        let base = self.pending.unwrap_or(current);
        let target = base + whole as f64 * WHEEL_STEP_DB;
        self.pending = Some(target.clamp(*range.start(), *range.end()));
    }

    pub fn due(&mut self, now: f64) -> Option<f64> {
        let idle = now - self.last_step >= WHEEL_IDLE_SECONDS;
        if self.pending.is_some() && idle {
            return self.pending.take();
        }
        None
    }

    /// When the pending wheel value becomes due, so the UI can wake up for it.
    pub fn deadline(&self) -> Option<f64> {
        self.pending.map(|_| self.last_step + WHEEL_IDLE_SECONDS)
    }

    pub fn shown(&self, device: f64) -> f64 {
        self.pending.unwrap_or(device)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_steps_debounce_to_one_value() {
        let range = output_level_range();
        let mut fader = FaderState::default();
        fader.on_wheel(1.0, 80.0, range.clone(), 1.0);
        fader.on_wheel(1.0, 80.0, range.clone(), 1.125);
        fader.on_wheel(1.0, 80.0, range, 1.25);
        assert_eq!(fader.shown(80.0), 81.5);
        assert_eq!(fader.due(1.5), None);
        assert_eq!(fader.due(1.625), Some(81.5));
        assert_eq!(fader.due(2.0), None);
        assert_eq!(fader.shown(80.0), 80.0);
    }

    #[test]
    fn wheel_deadline_follows_the_last_step_and_ends_when_due() {
        let range = output_level_range();
        let mut fader = FaderState::default();
        assert_eq!(fader.deadline(), None);
        fader.on_wheel(1.0, 80.0, range.clone(), 1.0);
        fader.on_wheel(1.0, 80.0, range, 1.5);
        assert_eq!(fader.deadline(), Some(1.5 + WHEEL_IDLE_SECONDS));
        fader.due(2.0);
        assert_eq!(fader.deadline(), None);
    }

    #[test]
    fn fractional_wheel_events_add_up_to_whole_notches() {
        let range = output_level_range();
        let mut fader = FaderState::default();
        for _ in 0..10 {
            fader.on_wheel(0.1, 80.0, range.clone(), 1.0);
        }
        assert_eq!(fader.shown(80.0), 80.5);
        fader.on_wheel(0.5, 80.0, range.clone(), 1.0);
        assert_eq!(fader.shown(80.0), 80.5);
        fader.on_wheel(0.5, 80.0, range, 1.0);
        assert_eq!(fader.shown(80.0), 81.0);
    }

    #[test]
    fn wheel_clamps_at_120() {
        let range = output_level_range();
        assert_eq!(range, 0.0..=120.0);
        let mut fader = FaderState::default();
        fader.on_wheel(10.0, 119.0, range.clone(), 0.0);
        assert_eq!(fader.due(1.0), Some(120.0));
        fader.on_wheel(-1000.0, 1.0, range, 2.0);
        assert_eq!(fader.due(3.0), Some(0.0));
    }
}
