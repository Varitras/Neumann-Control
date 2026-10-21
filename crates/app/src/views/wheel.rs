//! The mouse wheel over faders and knobs. A burst of wheel events belongs to the control under
//! the pointer where it began: a burst begun on a gap scrolls the area around it, and a knob or
//! fader that the scroll brings under the pointer takes none of its notches. A control holding
//! a burst stops the area around it from scrolling, so it stays under the pointer.

use eframe::egui::{Context, Event, Id, MouseWheelUnit, Pos2, Rect, Ui, Vec2};

use crate::gesture::WHEEL_IDLE_SECONDS;

const BURST_ID: &str = "wheel_burst";
/// Lines and pixels of a precision touchpad come as many small events, pages as rare large
/// ones; egui scrolls this many points per line.
const POINTS_PER_NOTCH: f32 = 40.0;
const NOTCHES_PER_PAGE: f32 = 10.0;

#[derive(Clone, Copy, Debug)]
struct Burst {
    /// Where the pointer was at the first event of the burst.
    start: Pos2,
    last_event: f64,
}

/// Once per frame before anything is drawn: notes where a new burst begins. Events closer
/// together than the wheel's idle time continue the burst.
pub fn track_burst(context: &Context) {
    let (wheeling, now, pointer) = context.input(|input| {
        let wheeling = input
            .events
            .iter()
            .any(|event| matches!(event, Event::MouseWheel { .. }));
        (wheeling, input.time, input.pointer.hover_pos())
    });
    let Some(pointer) = pointer.filter(|_| wheeling) else {
        return;
    };
    let id = Id::unique(BURST_ID);
    let previous = context.data(|data| data.get_temp::<Burst>(id));
    let start = previous
        .filter(|burst| now - burst.last_event < WHEEL_IDLE_SECONDS)
        .map_or(pointer, |burst| burst.start);
    let burst = Burst {
        start,
        last_event: now,
    };
    context.data_mut(|data| data.insert_temp(id, burst));
}

/// This frame's wheel events in notches (lines), upwards positive, for the control at `rect` when
/// the burst began over it; none otherwise.
pub(super) fn notches_for(ui: &Ui, rect: Rect) -> Vec<f64> {
    let burst = ui.data(|data| data.get_temp::<Burst>(Id::unique(BURST_ID)));
    let now = ui.input(|input| input.time);
    let held_here = burst.is_some_and(|burst| {
        rect.contains(burst.start) && now - burst.last_event < WHEEL_IDLE_SECONDS
    });
    if !held_here {
        return Vec::new();
    }
    // egui spreads a wheel event's scroll over several frames; all of them stay with the control.
    ui.input_mut(|input| input.smooth_scroll_delta = Vec2::ZERO);
    ui.input(|input| {
        input
            .events
            .iter()
            .filter_map(|event| match event {
                Event::MouseWheel { unit, delta, .. } if delta.y != 0.0 => {
                    Some(notches(*unit, delta.y))
                }
                _ => None,
            })
            .collect()
    })
}

fn notches(unit: MouseWheelUnit, delta: f32) -> f64 {
    let lines = match unit {
        MouseWheelUnit::Line => delta,
        MouseWheelUnit::Point => delta / POINTS_PER_NOTCH,
        MouseWheelUnit::Page => delta * NOTCHES_PER_PAGE,
    };
    f64::from(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_units_become_notches() {
        assert_eq!(notches(MouseWheelUnit::Line, 0.25), 0.25);
        assert_eq!(notches(MouseWheelUnit::Point, -80.0), -2.0);
        assert_eq!(notches(MouseWheelUnit::Page, 1.0), 10.0);
    }
}
