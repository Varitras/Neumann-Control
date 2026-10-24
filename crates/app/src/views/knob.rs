//! A rotary knob for a catalog number: turn it by moving the pointer around it (clockwise is
//! more), mouse wheel, double-click for exact entry. While it moves, the value floats beside it.
//! It writes once at the end of a gesture and shows a device value outside its range as is.

use std::f32::consts::PI;
use std::ops::RangeInclusive;

use eframe::egui::{
    pos2, vec2, Align, CursorIcon, Id, Key, Label, Layout, Pos2, RichText, Sense, Shape, Stroke,
    TextEdit, Ui, WidgetInfo,
};

use neumann_control_core::catalog::{Catalog, Conversion, Entry};
use neumann_control_core::speaker::Command;
use serde_json::Value;

use super::fader::{drag_aborted, value_beside, FINE_DIVISOR};
use super::wheel::notches_for;
use crate::display::{conversion_of, shown_value};
use crate::gesture::{bounded_move, released, typed_value, GestureState};
use crate::state::SpeakerState;
use crate::texts;
use crate::ui_kit::theme::Tokens;
use crate::writes::{self, Intent};

/// Closer to the centre than this the pointer's angle jumps around, so it is ignored.
const DEAD_ZONE_RADIUS: f32 = 4.0;
/// Wheel notches for the whole range, so wide ranges (delay in samples) stay usable.
const FULL_RANGE_NOTCHES: f64 = 100.0;
/// The ring leaves a gap at the bottom, from -135° to +135° around the top.
const SWEEP_RADIANS: f32 = 1.5 * PI;
const FULL_RANGE_TURN_RADIANS: f32 = 3.0 * PI;
const ARC_WIDTH: f32 = 3.0;
/// Space between the value arc and the knob's body.
const ARC_GAP: f32 = 2.0;
const BODY_BORDER_WIDTH: f32 = 1.0;
const INDICATOR_WIDTH: f32 = 2.0;
/// The pointer line runs from this fraction of the body radius outwards.
const INDICATOR_START: f32 = 0.3;
const ARC_SEGMENTS: usize = 32;
const DRAG_VALUE_ID: &str = "knob_drag_value";
const DRAG_ANGLE_ID: &str = "knob_drag_angle";
const DRAG_START_ID: &str = "knob_drag_start";
const EDIT_TEXT_ID: &str = "knob_edit_text";
const EDIT_WIDTH: f32 = 56.0;
/// Below the small text style, so the panel's names ("Verzögerung", "Standby-Zeit") fit their
/// column on one line; a longer one is cut, never broken inside a word, and shows whole on hover.
const CAPTION_SIZE: f32 = 9.0;
/// Room for the value line under the knob.
const CELL_MIN_WIDTH: f32 = 52.0;
pub const KNOB_DIAMETER: f32 = 34.0;
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
    // Through the write's own type, so a whole-number entry reads "60 min", not "60.0 min".
    let format = |value: f64| {
        let typed = writes::number_value(entry, reported.as_ref(), value);
        shown_value(entry, &typed)
    };
    let knob = Knob {
        label,
        caption,
        range: minimum..=maximum,
        step,
        device: reported.as_ref().and_then(Value::as_f64),
        enabled: writes::can_write(speaker, catalog, &entry.path),
        diameter,
        format: &format,
        conversion: conversion_of(entry),
    };
    let mut state = speaker.knobs.remove(&entry.path).unwrap_or_default();
    let id = ui.scope_id().with(entry.path.to_string());
    let sent = show(ui, id, &knob, &mut state);
    speaker.knobs.insert(entry.path.clone(), state);
    let sent = sent?;
    // A typed value outside the range is sent as typed, for the write policy to refuse.
    let value = if (minimum..=maximum).contains(&sent) {
        writes::number_value(entry, reported.as_ref(), sent)
    } else {
        Value::from(sent)
    };
    let now = ui.input(|input| input.time);
    let path = entry.path.clone();
    speaker.request(Intent::Write { path, value }, catalog, now)
}

pub struct Knob<'a> {
    /// Names the knob for screen readers and its tooltip.
    pub label: &'a str,
    /// Whether label and value stand under the knob; beside a number field they would repeat it.
    pub caption: bool,
    pub range: RangeInclusive<f64>,
    /// The catalog step; values sent are on this grid.
    pub step: f64,
    pub device: Option<f64>,
    pub enabled: bool,
    pub diameter: f32,
    /// Formats a value for the line under the knob.
    pub format: &'a dyn Fn(f64) -> String,
    /// The unit typed values are in, when it is not the device's.
    pub conversion: Option<Conversion>,
}

/// Draws the knob; returns the value to send when a gesture ended.
pub fn show(ui: &mut Ui, id: Id, knob: &Knob, state: &mut GestureState) -> Option<f64> {
    let tokens = Tokens::current(ui.ctx());
    let now = ui.input(|input| input.time);
    let device = knob.device.unwrap_or(*knob.range.start());
    let enabled = knob.enabled && knob.device.is_some();
    let mut sent = None;
    // A cell of its own width: a centred layout would take the whole row from its neighbours.
    let cell = vec2(knob.diameter.max(CELL_MIN_WIDTH), 0.0);
    let response = ui
        .allocate_ui_with_layout(cell, Layout::top_down(Align::Center), |ui| {
            let size = vec2(knob.diameter, knob.diameter);
            let sense = if enabled {
                Sense::click_and_drag()
            } else {
                Sense::hover()
            };
            let (rect, response) = ui.allocate_exact_size(size, sense);
            let drag_id = id.with(DRAG_VALUE_ID);
            let start_id = id.with(DRAG_START_ID);
            let angle_id = id.with(DRAG_ANGLE_ID);
            let pointer = ui.input(|input| input.pointer.interact_pos());
            let angle = pointer.and_then(|pointer| pointer_angle(rect.center(), pointer));
            let mut value = state.shown(device, now);
            if response.drag_started() {
                state.drag_started();
                ui.data_mut(|data| {
                    data.insert_temp(drag_id, value);
                    data.insert_temp(start_id, value);
                    data.remove::<f32>(angle_id);
                });
            }
            let aborted = drag_aborted(ui, &response);
            let started = ui
                .data(|data| data.get_temp::<f64>(start_id))
                .unwrap_or(value);
            if response.dragged() && !aborted {
                let held = ui
                    .data(|data| data.get_temp::<f64>(drag_id))
                    .unwrap_or(value);
                let last = ui.data(|data| data.get_temp::<f32>(angle_id));
                let fine = ui.input(|input| input.modifiers.shift);
                let turned = last
                    .zip(angle)
                    .map_or(0.0, |(last, angle)| angle_change(last, angle));
                let target = held + turn_change(&knob.range, turned, fine);
                value = bounded_move(started, target, &knob.range).unwrap_or(held);
                ui.data_mut(|data| {
                    data.insert_temp(drag_id, value);
                    // Inside the dead zone the last good angle stays the reference.
                    if let Some(angle) = angle {
                        data.insert_temp(angle_id, angle);
                    }
                });
            }
            if response.drag_stopped() && !aborted {
                let dragged = ui.data(|data| data.get_temp::<f64>(drag_id));
                sent = dragged.and_then(|dragged| released(started, dragged, knob.step));
            }
            if response.drag_stopped() {
                ui.data_mut(|data| {
                    data.remove::<f64>(drag_id);
                    data.remove::<f64>(start_id);
                });
            }
            // A held knob ignores the wheel, as a held fader does.
            let held = response.is_pointer_button_down_on() || response.dragged();
            if enabled && response.hovered() && !held {
                let wheel = notches_for(ui, rect);
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
            if enabled && response.hovered() {
                let icon = if response.dragged() {
                    CursorIcon::Grabbing
                } else {
                    CursorIcon::Grab
                };
                ui.ctx().set_cursor_icon(icon);
            }
            let moving = (response.dragged() && !aborted) || state.wheel_pending();
            if moving {
                value_beside(ui, id, rect, rect.center().y, &(knob.format)(value));
            }
            paint(ui, rect, knob, value, enabled, &tokens);
            let label = knob.label.to_owned();
            response.widget_info(|| WidgetInfo::slider(enabled, value, &label));
            if knob.caption {
                let caption = RichText::new(knob.label).size(CAPTION_SIZE).weak();
                ui.add(Label::new(caption).truncate());
            }
            let open_now = response.double_clicked();
            let edited = value_entry(ui, id, open_now, value, knob.conversion);
            sent = sent.or(edited);
            if knob.caption && !value_entry_open(ui, id) {
                let text = RichText::new((knob.format)(value)).small().monospace();
                ui.add(Label::new(text));
            }
            response
        })
        .inner;
    response.on_hover_text(format!("{}\n{}", knob.label, texts::CONTROL_HINT));
    sent
}

/// The pointer's angle around `center`, clockwise from the top; none too close to the centre.
pub fn pointer_angle(center: Pos2, pointer: Pos2) -> Option<f32> {
    let offset = pointer - center;
    if offset.length() < DEAD_ZONE_RADIUS {
        return None;
    }
    Some(offset.x.atan2(-offset.y))
}

/// The turn from `previous` to `current`, the short way round (across the bottom too).
pub fn angle_change(previous: f32, current: f32) -> f32 {
    let change = current - previous;
    if change > PI {
        return change - 2.0 * PI;
    }
    if change < -PI {
        return change + 2.0 * PI;
    }
    change
}

/// The value change for a turn of `radians`: one and a half turns cover the whole range, finer
/// than the drawn sweep so a value can be met.
pub fn turn_change(range: &RangeInclusive<f64>, radians: f32, fine: bool) -> f64 {
    let span = range.end() - range.start();
    let divisor = if fine { FINE_DIVISOR } else { 1.0 };
    f64::from(radians) / f64::from(FULL_RANGE_TURN_RADIANS) * span / divisor
}

/// One wheel notch: a whole number of catalog steps, at least one, about a hundredth of the
/// range.
pub fn wheel_step(range: &RangeInclusive<f64>, step: f64) -> f64 {
    let span = range.end() - range.start();
    let steps = (span / FULL_RANGE_NOTCHES / step).round().max(1.0);
    steps * step
}

/// A text field for an exact value, open from `open_now` until Enter (returns the value as typed,
/// see `gesture::typed_value`) or until Escape or leaving it (returns nothing). Knob and fader
/// readout use it. With a `conversion`, the text is in the converted unit (the delay in ms).
pub(super) fn value_entry(
    ui: &mut Ui,
    id: Id,
    open_now: bool,
    value: f64,
    conversion: Option<Conversion>,
) -> Option<f64> {
    let text_id = id.with(EDIT_TEXT_ID);
    if open_now {
        let shown = conversion.map_or(value, |conversion| conversion.apply(value));
        ui.data_mut(|data| data.insert_temp(text_id, texts::number(shown)));
    }
    let mut text = ui.data(|data| data.get_temp::<String>(text_id))?;
    let field = ui.add(
        TextEdit::singleline(&mut text)
            .id(text_id)
            .desired_width(EDIT_WIDTH),
    );
    // Asked of the drawn field: a request before it exists is lost to the opening click.
    if open_now {
        field.request_focus();
    }
    let entered = field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
    if field.lost_focus() {
        ui.data_mut(|data| data.remove::<String>(text_id));
    } else {
        ui.data_mut(|data| data.insert_temp(text_id, text.clone()));
    }
    if !entered {
        return None;
    }
    typed_value(&text, conversion)
}

pub(super) fn value_entry_open(ui: &Ui, id: Id) -> bool {
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
    let arc_radius = knob.diameter / 2.0 - ARC_WIDTH / 2.0;
    let body_radius = arc_radius - ARC_WIDTH / 2.0 - ARC_GAP;
    let span = knob.range.end() - knob.range.start();
    let position = ((value - knob.range.start()) / span).clamp(0.0, 1.0) as f32;
    let start = -SWEEP_RADIANS / 2.0;
    let angle = start + position * SWEEP_RADIANS;
    let on_circle = |radius: f32, angle: f32| center + radius * vec2(angle.sin(), -angle.cos());
    let arc = |from: f32, to: f32| -> Vec<_> {
        (0..=ARC_SEGMENTS)
            .map(|segment| {
                on_circle(
                    arc_radius,
                    from + (to - from) * segment as f32 / ARC_SEGMENTS as f32,
                )
            })
            .collect()
    };
    let end = start + SWEEP_RADIANS;
    painter.add(Shape::line(
        arc(start, end),
        Stroke::new(ARC_WIDTH, tokens.meter_track),
    ));
    let color = if enabled {
        tokens.accent
    } else {
        tokens.text_dim
    };
    painter.add(Shape::line(
        arc(start, angle),
        Stroke::new(ARC_WIDTH, color),
    ));
    painter.circle(
        center,
        body_radius,
        tokens.field,
        Stroke::new(BODY_BORDER_WIDTH, tokens.border),
    );
    let tip = on_circle(body_radius, angle);
    let inner = on_circle(body_radius * INDICATOR_START, angle);
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
    fn pointer_angle_is_measured_clockwise_from_the_top() {
        let center = pos2(50.0, 50.0);
        assert_eq!(pointer_angle(center, pos2(50.0, 20.0)), Some(0.0));
        assert_eq!(pointer_angle(center, pos2(80.0, 50.0)), Some(PI / 2.0));
        assert_eq!(
            pointer_angle(center, pos2(52.0, 51.0)),
            None,
            "too close to the centre"
        );
    }

    #[test]
    fn turning_takes_the_short_way_across_the_bottom() {
        let small = 0.1;
        assert!((angle_change(PI - small, -PI + small) - 2.0 * small).abs() < 1e-5);
        assert!((angle_change(-PI + small, PI - small) + 2.0 * small).abs() < 1e-5);
    }

    #[test]
    fn one_and_a_half_turns_cover_the_range_and_shift_is_finer() {
        assert_eq!(turn_change(&DELAY_RANGE, 3.0 * PI, false), 5760.0);
        assert!((turn_change(&DELAY_RANGE, 3.0 * PI, true) - 576.0).abs() < 1e-9);
    }

    #[test]
    fn a_wheel_notch_is_a_whole_number_of_steps() {
        assert_eq!(wheel_step(&DELAY_RANGE, 1.0), 58.0);
        assert_eq!(wheel_step(&(1.0..=240.0), 1.0), 2.0);
        assert_eq!(wheel_step(&(0.0..=10.0), 1.0), 1.0);
    }
}
