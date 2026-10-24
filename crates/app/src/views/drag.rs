//! One drag of a value control (a fader, a knob): the value it began at, the value it holds while
//! the pointer moves, and what the release writes. Escape during the drag writes nothing.

use std::ops::RangeInclusive;

use eframe::egui::{Id, Key, Response, Ui};

use crate::gesture::{self, bounded_move};

const ABORTED_ID: &str = "drag_aborted";
const HELD_ID: &str = "drag_held";
const START_ID: &str = "drag_start";

/// A drag this frame.
pub(super) struct Drag {
    /// The value when the drag began; a group move is measured from it, not from the reference
    /// of the release frame, which may have changed meanwhile.
    pub started: f64,
    /// While held and not escaped: the value the pointer moved to, off the grid.
    pub current: Option<f64>,
    /// On release: the value to write, on the grid, and only when it moved from the start on
    /// the grid.
    pub released: Option<f64>,
}

/// Follows the drag on `response`. `start` is the value when a drag begins, `reach` how far it
/// may go, `step` the grid of the release; `change` is the value change the pointer made this
/// frame, asked only while it drags. `key` keeps the drag's values apart from other controls'.
pub(super) fn follow(
    ui: &Ui,
    response: &Response,
    key: Id,
    start: f64,
    reach: &RangeInclusive<f64>,
    step: f64,
    change: impl FnOnce() -> f64,
) -> Drag {
    let held_id = key.with(HELD_ID);
    let start_id = key.with(START_ID);
    if response.drag_started() {
        ui.data_mut(|data| {
            data.insert_temp(held_id, start);
            data.insert_temp(start_id, start);
        });
    }
    let aborted = escaped(ui, response);
    let started = ui
        .data(|data| data.get_temp::<f64>(start_id))
        .unwrap_or(start);
    let mut current = None;
    if response.dragged() && !aborted {
        let held = ui
            .data(|data| data.get_temp::<f64>(held_id))
            .unwrap_or(start);
        let moved = bounded_move(started, held + change(), reach).unwrap_or(held);
        ui.data_mut(|data| data.insert_temp(held_id, moved));
        current = Some(moved);
    }
    let mut released = None;
    if response.drag_stopped() {
        let dragged = ui.data(|data| data.get_temp::<f64>(held_id));
        ui.data_mut(|data| {
            data.remove::<f64>(held_id);
            data.remove::<f64>(start_id);
        });
        released = dragged
            .filter(|_| !aborted)
            .and_then(|dragged| gesture::released(started, dragged, step));
    }
    Drag {
        started,
        current,
        released,
    }
}

/// Escape ends egui's drag but leaves the button down, and the control keeps following the
/// pointer until it is released; none of that may reach the speaker. True from the Escape
/// frame through the release frame.
fn escaped(ui: &Ui, response: &Response) -> bool {
    let id = response.id.with(ABORTED_ID);
    let was_aborted = ui.data(|data| data.get_temp::<bool>(id)).unwrap_or(false);
    let escape = ui.input(|input| input.key_pressed(Key::Escape));
    let aborted = was_aborted || (escape && response.drag_stopped());
    let button_down = ui.input(|input| input.pointer.primary_down());
    ui.data_mut(|data| data.insert_temp(id, aborted && button_down));
    aborted
}
