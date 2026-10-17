//! The card's level fader: a vertical slider that writes once it settles, and the mouse wheel
//! in 0.5 dB steps per notch.

use std::ops::RangeInclusive;

use eframe::egui::{Event, Key, MouseWheelUnit, Response, Slider, Ui, Vec2};
use neumann_control_core::catalog::Catalog;
use neumann_control_core::speaker::Command;

use crate::fader::WHEEL_STEP_DB;
use crate::paths;
use crate::state::SpeakerState;
use crate::writes::{self, Intent};

const DRAG_ABORTED_ID: &str = "fader_drag_aborted";

/// The slider's length.
pub const FADER_HEIGHT: f32 = 150.0;

/// The vertical level fader; returns the value it shows.
pub fn show(
    ui: &mut Ui,
    speaker: &mut SpeakerState,
    catalog: &Catalog,
    commands: &mut Vec<Command>,
    device: Option<f64>,
    range: RangeInclusive<f64>,
) -> f64 {
    let path = paths::path(paths::OUTPUT_LEVEL);
    let enabled = device.is_some() && writes::can_write(speaker, catalog, &path);
    let device = device.unwrap_or(*range.start());
    let now = ui.input(|input| input.time);
    let mut value = speaker.fader.shown(device, now);
    ui.spacing_mut().slider_width = FADER_HEIGHT;
    let slider = Slider::new(&mut value, range.clone())
        .vertical()
        .show_value(false)
        .step_by(WHEEL_STEP_DB);
    let response = ui.add_enabled(enabled, slider);
    if response.drag_started() {
        speaker.fader.drag_started();
    }
    let aborted = drag_aborted(ui, &response);
    if aborted {
        value = speaker.fader.shown(device, now);
    }
    // While dragging, the slider follows the pointer and the speaker hears nothing. The release
    // frame still moves the value but no longer counts as dragged, so this one condition sends
    // the final value of a drag, a click on the track and a key press alike.
    let settled = response.changed() && !response.dragged() && !aborted;
    if settled {
        commands.extend(speaker.request(Intent::Level(value), catalog, now));
    }
    if enabled && response.hovered() {
        // The cards sit in a scroll area, which would scroll along with the fader.
        ui.input_mut(|input| input.smooth_scroll_delta = Vec2::ZERO);
        for notches in wheel_notches(ui) {
            speaker.fader.on_wheel(notches, device, range.clone(), now);
        }
        value = speaker.fader.shown(device, now);
    }
    if let Some(due) = speaker.fader.due(now) {
        commands.extend(speaker.request(Intent::Level(due), catalog, now));
    }
    if let Some(deadline) = speaker.fader.deadline() {
        let wait = (deadline - now).max(0.0);
        ui.ctx().request_repaint_after_secs(wait as f32);
    }
    value
}

/// Escape ends egui's drag but leaves the button down, and the slider keeps following the
/// pointer until it is released; none of that may reach the speaker. True from the Escape
/// frame through the release frame.
fn drag_aborted(ui: &Ui, response: &Response) -> bool {
    let id = response.id.with(DRAG_ABORTED_ID);
    let was_aborted = ui.data(|data| data.get_temp::<bool>(id)).unwrap_or(false);
    let escape = ui.input(|input| input.key_pressed(Key::Escape));
    let aborted = was_aborted || (escape && response.drag_stopped());
    let button_down = ui.input(|input| input.pointer.primary_down());
    ui.data_mut(|data| data.insert_temp(id, aborted && button_down));
    aborted
}

/// Lines and pixels of a precision touchpad come as many small events, pages as rare large
/// ones; egui scrolls this many points per line.
const POINTS_PER_NOTCH: f32 = 40.0;
const NOTCHES_PER_PAGE: f32 = 10.0;

/// Each wheel event of this frame in notches (lines), upwards positive.
fn wheel_notches(ui: &Ui) -> Vec<f64> {
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
