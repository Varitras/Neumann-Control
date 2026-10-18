//! What a fader or knob shows and sends between gestures: wheel steps collect until the wheel
//! goes quiet, then one write goes out; a sent value is shown until the speaker confirms it.
//! Also the rules every control shares for what a gesture may write: a drag writes only when it
//! changed the value, a move never leads further out of the range, and a typed value is taken
//! as typed (the write policy refuses one outside the range). Times are seconds of the UI clock.

use std::ops::RangeInclusive;

use neumann_control_core::catalog::Conversion;

pub const WHEEL_IDLE_SECONDS: f64 = 0.3;
/// egui may wake a frame about 16 ms before the requested time; within this much of the
/// deadline the wheel value counts as due instead of booking another tiny wait.
pub const DUE_SLACK_SECONDS: f64 = 0.05;
/// How long the fader keeps showing a sent value while the speaker has not confirmed it.
pub const SENT_HOLD_SECONDS: f64 = 2.0;
/// Fractions of a notch are counted in whole units so ten tenths make exactly one notch.
const NOTCH_RESOLUTION: f64 = 1000.0;
/// A drawn control asks for its wheel step about `WHEEL_IDLE_SECONDS` after the last notch. Asked
/// this much later, the control was hidden meanwhile (panel closed, mode switched), and the step
/// is dropped instead of being written without a gesture.
const STALE_WHEEL_SECONDS: f64 = 1.0;

#[derive(Clone, Copy, Debug, Default)]
pub struct GestureState {
    pending: Option<f64>,
    last_step: f64,
    /// Wheel movement short of a whole notch, in 1/NOTCH_RESOLUTION notches.
    partial_notch: i64,
    /// The last value sent and when, shown until the speaker reports the level again.
    sent: Option<(f64, f64)>,
}

impl GestureState {
    /// `notches` may be fractional (touchpads, high-resolution wheels); one `step` per whole
    /// notch, the rest is kept for the next event. `range` is the allowed value range of the
    /// controlled entry.
    pub fn on_wheel(
        &mut self,
        notches: f64,
        step: f64,
        current: f64,
        range: RangeInclusive<f64>,
        now: f64,
    ) {
        self.partial_notch += (notches * NOTCH_RESOLUTION).round() as i64;
        let resolution = NOTCH_RESOLUTION as i64;
        let whole = self.partial_notch / resolution;
        self.partial_notch %= resolution;
        self.last_step = now;
        if whole == 0 {
            return;
        }
        let base = self.pending.unwrap_or(current);
        let target = base + whole as f64 * step;
        if let Some(moved) = bounded_move(base, target, &range) {
            self.pending = Some(moved);
        }
    }

    /// Wheel steps wait to be sent.
    pub fn wheel_pending(&self) -> bool {
        self.pending.is_some()
    }

    pub fn due(&mut self, now: f64) -> Option<f64> {
        if now - self.last_step > STALE_WHEEL_SECONDS {
            self.pending = None;
        }
        let idle = now - self.last_step >= WHEEL_IDLE_SECONDS - DUE_SLACK_SECONDS;
        if self.pending.is_some() && idle {
            return self.pending.take();
        }
        None
    }

    /// A drag takes over from the wheel: steps not yet sent would overwrite its value.
    pub fn drag_started(&mut self) {
        self.pending = None;
        self.partial_notch = 0;
    }

    pub fn sent(&mut self, value: f64, now: f64) {
        self.drag_started();
        self.sent = Some((value, now));
    }

    /// The speaker reported the level; from now on its value is shown.
    pub fn confirmed(&mut self) {
        self.sent = None;
    }

    /// When the shown value changes on its own (the wheel becomes due, a sent value stops
    /// being held), so the UI can wake up for it.
    pub fn deadline(&self) -> Option<f64> {
        let wheel = self.pending.map(|_| self.last_step + WHEEL_IDLE_SECONDS);
        let hold = self.sent.map(|(_, at)| at + SENT_HOLD_SECONDS);
        wheel.into_iter().chain(hold).reduce(f64::min)
    }

    pub fn shown(&self, device: f64, now: f64) -> f64 {
        let held = self
            .sent
            .filter(|(_, at)| now - at < SENT_HOLD_SECONDS)
            .map(|(value, _)| value);
        self.pending.or(held).unwrap_or(device)
    }
}

/// Where a move from `start` to `target` may land in `range`. From inside, a move past an end
/// stops there. From a device value outside the range, only a move back toward the range counts
/// and lands at its near end; a move further out goes nowhere.
pub fn bounded_move(start: f64, target: f64, range: &RangeInclusive<f64>) -> Option<f64> {
    let inside = range.contains(&start);
    let inward = distance(target, range) < distance(start, range);
    (inside || inward).then(|| target.clamp(*range.start(), *range.end()))
}

fn distance(value: f64, range: &RangeInclusive<f64>) -> f64 {
    (range.start() - value).max(value - range.end()).max(0.0)
}

/// What a drag from `start` to `end` writes: the end on the control's grid, and only when that
/// differs from the start on the grid, so a click or a drag back to the start writes nothing.
pub fn released(start: f64, end: f64, step: f64) -> Option<f64> {
    let end = on_grid(end, step);
    (end != on_grid(start, step)).then_some(end)
}

pub fn on_grid(value: f64, step: f64) -> f64 {
    (value / step).round() * step
}

/// A value typed into a control: a point or a comma as decimal separator, converted from the unit
/// it was typed in. Not fitted to the range: the write policy refuses a value outside it and says
/// why, so a typo never becomes the range's end.
pub fn typed_value(text: &str, conversion: Option<Conversion>) -> Option<f64> {
    let typed: f64 = text.trim().replace(',', ".").parse().ok()?;
    Some(conversion.map_or(typed, |conversion| conversion.invert(typed)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEVEL_STEP: f64 = 0.5;
    const LEVEL_RANGE: RangeInclusive<f64> = 0.0..=120.0;

    #[test]
    fn wheel_steps_debounce_to_one_value() {
        let range = LEVEL_RANGE;
        let mut fader = GestureState::default();
        fader.on_wheel(1.0, LEVEL_STEP, 80.0, range.clone(), 1.0);
        fader.on_wheel(1.0, LEVEL_STEP, 80.0, range.clone(), 1.125);
        fader.on_wheel(1.0, LEVEL_STEP, 80.0, range, 1.25);
        assert_eq!(fader.shown(80.0, 0.0), 81.5);
        assert_eq!(fader.due(1.375), None);
        assert_eq!(fader.due(1.625), Some(81.5));
        assert_eq!(fader.due(2.0), None);
        assert_eq!(fader.shown(80.0, 0.0), 80.0);
    }

    #[test]
    fn wheel_deadline_follows_the_last_step_and_ends_when_due() {
        let range = LEVEL_RANGE;
        let mut fader = GestureState::default();
        assert_eq!(fader.deadline(), None);
        fader.on_wheel(1.0, LEVEL_STEP, 80.0, range.clone(), 1.0);
        fader.on_wheel(1.0, LEVEL_STEP, 80.0, range, 1.5);
        assert_eq!(fader.deadline(), Some(1.5 + WHEEL_IDLE_SECONDS));
        fader.due(2.0);
        assert_eq!(fader.deadline(), None);
    }

    #[test]
    fn fractional_wheel_events_add_up_to_whole_notches() {
        let range = LEVEL_RANGE;
        let mut fader = GestureState::default();
        for _ in 0..10 {
            fader.on_wheel(0.1, LEVEL_STEP, 80.0, range.clone(), 1.0);
        }
        assert_eq!(fader.shown(80.0, 0.0), 80.5);
        fader.on_wheel(0.5, LEVEL_STEP, 80.0, range.clone(), 1.0);
        assert_eq!(fader.shown(80.0, 0.0), 80.5);
        fader.on_wheel(0.5, LEVEL_STEP, 80.0, range, 1.0);
        assert_eq!(fader.shown(80.0, 0.0), 81.0);
    }

    #[test]
    fn the_wheel_is_due_within_the_slack_before_its_deadline() {
        let range = LEVEL_RANGE;
        let mut fader = GestureState::default();
        fader.on_wheel(1.0, LEVEL_STEP, 80.0, range, 1.0);
        assert_eq!(fader.due(1.125), None);
        assert_eq!(fader.due(1.25), Some(80.5));
    }

    #[test]
    fn a_drag_drops_wheel_steps_not_yet_sent() {
        let range = LEVEL_RANGE;
        let mut fader = GestureState::default();
        fader.on_wheel(2.0, LEVEL_STEP, 80.0, range, 1.0);
        fader.drag_started();
        assert_eq!(fader.due(5.0), None);
        assert_eq!(fader.deadline(), None);
    }

    #[test]
    fn a_sent_value_is_shown_until_confirmed_or_two_seconds_passed() {
        let range = LEVEL_RANGE;
        let mut fader = GestureState::default();
        fader.on_wheel(1.0, LEVEL_STEP, 80.0, range, 1.0);
        fader.sent(90.0, 1.0);
        assert_eq!(fader.due(5.0), None);
        assert_eq!(fader.shown(80.0, 2.5), 90.0);
        assert_eq!(fader.deadline(), Some(1.0 + SENT_HOLD_SECONDS));
        assert_eq!(fader.shown(80.0, 3.0), 80.0);
        fader.sent(95.0, 4.0);
        fader.confirmed();
        assert_eq!(fader.shown(80.0, 4.0), 80.0);
        assert_eq!(fader.deadline(), None);
    }

    #[test]
    fn wheel_clamps_at_the_range_ends() {
        let range = LEVEL_RANGE;
        let mut fader = GestureState::default();
        fader.on_wheel(10.0, LEVEL_STEP, 119.0, range.clone(), 0.0);
        assert_eq!(fader.due(1.0), Some(120.0));
        fader.on_wheel(-1000.0, LEVEL_STEP, 1.0, range, 2.0);
        assert_eq!(fader.due(3.0), Some(0.0));
    }

    #[test]
    fn wheel_moves_one_step_per_notch() {
        let mut knob = GestureState::default();
        knob.on_wheel(2.0, 1.0, 10.0, 0.0..=100.0, 0.0);
        assert_eq!(knob.due(1.0), Some(12.0));
        knob.on_wheel(-1.0, 5.0, 12.0, 0.0..=100.0, 2.0);
        assert_eq!(knob.due(3.0), Some(7.0));
    }

    const DELAY_RANGE: RangeInclusive<f64> = 0.0..=5760.0;

    #[test]
    fn a_wheel_step_whose_control_was_not_drawn_in_time_is_dropped() {
        let mut fader = GestureState::default();
        fader.on_wheel(1.0, LEVEL_STEP, 80.0, LEVEL_RANGE, 1.0);
        // A drawn control asks within the idle time; much later it was hidden meanwhile.
        assert_eq!(fader.due(4.0), None);
        assert_eq!(fader.deadline(), None);
    }

    #[test]
    fn a_drag_outside_the_range_only_moves_back_into_it() {
        assert_eq!(bounded_move(6000.0, 6100.0, &DELAY_RANGE), None);
        assert_eq!(bounded_move(6000.0, 5900.0, &DELAY_RANGE), Some(5760.0));
        assert_eq!(bounded_move(5000.0, 6100.0, &DELAY_RANGE), Some(5760.0));
        assert_eq!(bounded_move(5000.0, 4000.0, &DELAY_RANGE), Some(4000.0));
    }

    #[test]
    fn a_drag_writes_only_what_it_changed_on_the_grid() {
        assert_eq!(released(90.3, 90.3, LEVEL_STEP), None, "a click");
        assert_eq!(
            released(90.3, 90.4, LEVEL_STEP),
            None,
            "still 90.5 on the grid"
        );
        assert_eq!(released(90.3, 91.1, LEVEL_STEP), Some(91.0));
    }

    #[test]
    fn a_typed_value_is_taken_as_typed_and_converted() {
        assert_eq!(typed_value(" 87,25 ", None), Some(87.25));
        assert_eq!(typed_value("1000", None), Some(1000.0));
        assert_eq!(
            typed_value("10", Some(Conversion::SamplesToMilliseconds)),
            Some(480.0)
        );
        assert_eq!(typed_value("laut", None), None);
    }

    #[test]
    fn sent_values_sit_on_the_step_grid() {
        assert_eq!(on_grid(12.4, 1.0), 12.0);
        assert_eq!(on_grid(1.26, 0.5), 1.5);
    }

    #[test]
    fn a_wheel_outside_the_range_only_moves_back_into_it() {
        let mut knob = GestureState::default();
        knob.on_wheel(1.0, 58.0, 6000.0, DELAY_RANGE, 0.0);
        assert_eq!(knob.due(0.5), None, "a step further out writes nothing");
        knob.on_wheel(-1.0, 58.0, 6000.0, DELAY_RANGE, 1.0);
        assert_eq!(knob.due(1.5), Some(5760.0));
    }
}
