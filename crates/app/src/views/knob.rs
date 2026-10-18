//! A rotary knob for a catalog number: vertical drag, mouse wheel, double-click for exact entry.
//! It writes once at the end of a gesture and shows a device value outside its range as is.

use std::f32::consts::PI;
use std::ops::RangeInclusive;

use eframe::egui::{
    pos2, vec2, Id, Key, Label, RichText, Sense, Shape, Stroke, TextEdit, Ui, WidgetInfo,
};

use neumann_control_core::catalog::{Catalog, Entry};
use neumann_control_core::speaker::Command;
use serde_json::Value;

use super::fader::{drag_aborted, wheel_notches};
use crate::display::shown_value;
use crate::gesture::GestureState;
use crate::state::SpeakerState;
use crate::ui_kit::theme::Tokens;
use crate::writes::{self, Intent};

/// Points of vertical drag for the whole range; Shift makes it this many times finer.
const FULL_RANGE_DRAG_POINTS: f64 = 200.0;
const FINE_DIVISOR: f64 = 10.0;
/// Wheel notches for the whole range, so wide ranges (delay in samples) stay usable.
const FULL_RANGE_NOTCHES: f64 = 100.0;
/// The ring leaves a gap at the bottom, from -135° to +135° around the top.
const SWEEP_RADIANS: f32 = 1.5 * PI;
const RING_WIDTH: f32 = 2.0;
const INDICATOR_WIDTH: f32 = 2.0;
const ARC_SEGMENTS: usize = 32;
const DRAG_VALUE_ID: &str = "knob_drag_value";
const EDIT_TEXT_ID: &str = "knob_edit_text";
const EDIT_WIDTH: f32 = 56.0;
/// Grid for entries without a catalog step.
const UNSTEPPED_DIVISIONS: f64 = 1000.0;

/// A knob for a catalog number of the speaker; returns the write the policy allowed.
pub fn for_entry(
    ui: &mut Ui,
    speaker: &mut SpeakerState,
    catalog: &Catalog,
    entry: &Entry,
    label: &str,
    caption: bool,
    diameter: f32,
) -> Option<Command> {
    let product = speaker.product().to_owned();
    let minimum = entry.min?;
    let maximum = entry.max_for(&product, &catalog.classes)?;
    let reported = speaker.values.get(&entry.path).cloned();
    let step = entry
        .step
        .unwrap_or((maximum - minimum) / UNSTEPPED_DIVISIONS);
    let format = |value: f64| shown_value(entry, &Value::from(value));
    let knob = Knob {
        label,
        caption,
        range: minimum..=maximum,
        step,
        device: reported.as_ref().and_then(Value::as_f64),
        enabled: writes::can_write(speaker, catalog, &entry.path),
        diameter,
        format: &format,
    };
    let mut state = speaker.knobs.remove(&entry.path).unwrap_or_default();
    let id = ui.id().with(entry.path.to_string());
    let sent = show(ui, id, &knob, &mut state);
    speaker.knobs.insert(entry.path.clone(), state);
    let value = writes::number_value(entry, reported.as_ref(), sent?);
    let now = ui.input(|input| input.time);
    let path = entry.path.clone();
    speaker.request(Intent::Write { path, value }, catalog, now)
}

pub struct Knob<'a> {
    /// Names the knob for screen readers and its tooltip.
    pub label: &'a str,
    /// Whether the label also stands under the knob.
    pub caption: bool,
    pub range: RangeInclusive<f64>,
    /// The catalog step; values sent are on this grid.
    pub step: f64,
    pub device: Option<f64>,
    pub enabled: bool,
    pub diameter: f32,
    /// Formats a value for the line under the knob.
    pub format: &'a dyn Fn(f64) -> String,
}

/// Draws the knob; returns the value to send when a gesture ended.
pub fn show(ui: &mut Ui, id: Id, knob: &Knob, state: &mut GestureState) -> Option<f64> {
    let tokens = Tokens::current(ui.ctx());
    let now = ui.input(|input| input.time);
    let device = knob.device.unwrap_or(*knob.range.start());
    let enabled = knob.enabled && knob.device.is_some();
    let mut sent = None;
    let response = ui
        .vertical_centered(|ui| {
            let size = vec2(knob.diameter, knob.diameter);
            let sense = if enabled {
                Sense::click_and_drag()
            } else {
                Sense::hover()
            };
            let (rect, response) = ui.allocate_exact_size(size, sense);
            let drag_id = id.with(DRAG_VALUE_ID);
            let mut value = state.shown(device, now);
            if response.drag_started() {
                state.drag_started();
                ui.data_mut(|data| data.insert_temp(drag_id, value));
            }
            let aborted = drag_aborted(ui, &response);
            if response.dragged() && !aborted {
                let held = ui
                    .data(|data| data.get_temp::<f64>(drag_id))
                    .unwrap_or(value);
                let fine = ui.input(|input| input.modifiers.shift);
                let moved = drag_change(&knob.range, -f64::from(response.drag_delta().y), fine);
                value = (held + moved).clamp(*knob.range.start(), *knob.range.end());
                ui.data_mut(|data| data.insert_temp(drag_id, value));
            }
            if response.drag_stopped() && !aborted {
                let dragged = ui.data(|data| data.get_temp::<f64>(drag_id));
                sent = dragged.map(|value| on_grid(value, knob.step));
            }
            if response.drag_stopped() {
                ui.data_mut(|data| data.remove::<f64>(drag_id));
            }
            if enabled && response.hovered() {
                let wheel = wheel_notches(ui);
                let step = wheel_step(&knob.range, knob.step);
                for notches in &wheel {
                    state.on_wheel(*notches, step, device, knob.range.clone(), now);
                }
                if !wheel.is_empty() {
                    value = state.shown(device, now);
                }
            }
            if let Some(due) = state.due(now) {
                sent = Some(due);
            }
            if let Some(deadline) = state.deadline() {
                ui.ctx()
                    .request_repaint_after_secs((deadline - now).max(0.0) as f32);
            }
            paint(ui, rect, knob, value, enabled, &tokens);
            let label = knob.label.to_owned();
            response.widget_info(|| WidgetInfo::slider(enabled, value, &label));
            if knob.caption {
                ui.add(Label::new(RichText::new(knob.label).small().weak()));
            }
            let edited = exact_entry(ui, id, &response, knob, value);
            sent = sent.or(edited);
            if exact_entry_open(ui, id) {
                return response;
            }
            ui.add(Label::new(
                RichText::new((knob.format)(value)).small().monospace(),
            ));
            response
        })
        .inner;
    response.on_hover_text(knob.label);
    if let Some(value) = sent {
        state.sent(value, now);
    }
    sent
}

/// The value change for a vertical drag of `points` (upwards positive).
pub fn drag_change(range: &RangeInclusive<f64>, points: f64, fine: bool) -> f64 {
    let span = range.end() - range.start();
    let per_point = span / FULL_RANGE_DRAG_POINTS;
    let divisor = if fine { FINE_DIVISOR } else { 1.0 };
    points * per_point / divisor
}

/// One wheel notch: a whole number of catalog steps, at least one, about a hundredth of the
/// range.
pub fn wheel_step(range: &RangeInclusive<f64>, step: f64) -> f64 {
    let span = range.end() - range.start();
    let steps = (span / FULL_RANGE_NOTCHES / step).round().max(1.0);
    steps * step
}

pub fn on_grid(value: f64, step: f64) -> f64 {
    (value / step).round() * step
}

/// Double-click opens a text field under the knob; Enter sends, Escape or leaving cancels.
fn exact_entry(
    ui: &mut Ui,
    id: Id,
    response: &eframe::egui::Response,
    knob: &Knob,
    value: f64,
) -> Option<f64> {
    let text_id = id.with(EDIT_TEXT_ID);
    if response.double_clicked() {
        ui.data_mut(|data| data.insert_temp(text_id, value.to_string()));
        ui.memory_mut(|memory| memory.request_focus(text_id));
    }
    let mut text = ui.data(|data| data.get_temp::<String>(text_id))?;
    let field = ui.add(
        TextEdit::singleline(&mut text)
            .id(text_id)
            .desired_width(EDIT_WIDTH),
    );
    let entered = field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
    if field.lost_focus() {
        ui.data_mut(|data| data.remove::<String>(text_id));
    } else {
        ui.data_mut(|data| data.insert_temp(text_id, text.clone()));
    }
    if !entered {
        return None;
    }
    let typed: f64 = text.trim().replace(',', ".").parse().ok()?;
    let clamped = typed.clamp(*knob.range.start(), *knob.range.end());
    Some(on_grid(clamped, knob.step))
}

fn exact_entry_open(ui: &Ui, id: Id) -> bool {
    ui.data(|data| data.get_temp::<String>(id.with(EDIT_TEXT_ID)))
        .is_some()
}

fn paint(
    ui: &Ui,
    rect: eframe::egui::Rect,
    knob: &Knob,
    value: f64,
    enabled: bool,
    tokens: &Tokens,
) {
    let painter = ui.painter();
    let center = rect.center();
    let radius = knob.diameter / 2.0 - RING_WIDTH;
    let span = knob.range.end() - knob.range.start();
    let position = ((value - knob.range.start()) / span).clamp(0.0, 1.0) as f32;
    let start = -SWEEP_RADIANS / 2.0;
    let angle = start + position * SWEEP_RADIANS;
    let point = |angle: f32| center + radius * vec2(angle.sin(), -angle.cos());
    let track: Vec<_> = (0..=ARC_SEGMENTS)
        .map(|segment| point(start + SWEEP_RADIANS * segment as f32 / ARC_SEGMENTS as f32))
        .collect();
    painter.add(Shape::line(
        track,
        Stroke::new(RING_WIDTH, tokens.meter_track),
    ));
    let color = if enabled {
        tokens.accent
    } else {
        tokens.text_dim
    };
    let filled: Vec<_> = (0..=ARC_SEGMENTS)
        .map(|segment| point(start + (angle - start) * segment as f32 / ARC_SEGMENTS as f32))
        .collect();
    painter.add(Shape::line(filled, Stroke::new(RING_WIDTH, color)));
    let tip = point(angle);
    let inner = center + (tip - center) * 0.35;
    painter.line_segment(
        [pos2(inner.x, inner.y), tip],
        Stroke::new(INDICATOR_WIDTH, tokens.text),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const DELAY_RANGE: RangeInclusive<f64> = 0.0..=5760.0;

    #[test]
    fn a_full_drag_covers_the_range_and_shift_is_finer() {
        assert_eq!(
            drag_change(&DELAY_RANGE, FULL_RANGE_DRAG_POINTS, false),
            5760.0
        );
        assert_eq!(drag_change(&DELAY_RANGE, 10.0, true), 28.8);
    }

    #[test]
    fn a_wheel_notch_is_a_whole_number_of_steps() {
        assert_eq!(wheel_step(&DELAY_RANGE, 1.0), 58.0);
        assert_eq!(wheel_step(&(1.0..=240.0), 1.0), 2.0);
        assert_eq!(wheel_step(&(0.0..=10.0), 1.0), 1.0);
    }

    #[test]
    fn sent_values_sit_on_the_step_grid() {
        assert_eq!(on_grid(12.4, 1.0), 12.0);
        assert_eq!(on_grid(1.26, 0.5), 1.5);
    }
}
