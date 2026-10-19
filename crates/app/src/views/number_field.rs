//! A number field as one gesture: a drag or a typed value is held while it lasts and handed out
//! once, on the frame it ends (release, Enter or focus loss); Escape drops it. The device value
//! is never clamped or written back. The device panel and the EQ table use it.

use std::ops::RangeInclusive;

use eframe::egui::{DragValue, Event, Id, Key, Response, Ui};
use neumann_control_core::catalog::Conversion;

use crate::display::converted_unit;
use crate::gesture::{bounded_move, on_grid, released, typed_value};
use crate::texts;

const PENDING_NUMBER_ID: &str = "pending_number";

fn is_text_edit(event: &Event) -> bool {
    match event {
        Event::Text(_) | Event::Paste(_) | Event::Cut => true,
        Event::Key {
            key, pressed: true, ..
        } => matches!(key, Key::Backspace | Key::Delete),
        _ => false,
    }
}

pub struct NumberField<'a> {
    pub id: Id,
    /// What the speaker reports; `None` before the first report.
    pub device: Option<f64>,
    /// Where a drag may go; a typed value is handed out as typed, for the write policy.
    pub range: RangeInclusive<f64>,
    /// The device's grid, if it has one: a gesture ending on the start's grid point is no change.
    pub step: Option<f64>,
    pub drag_speed: f64,
    pub unit: Option<&'a str>,
    pub conversion: Option<Conversion>,
    pub enabled: bool,
    /// The label the field is named by for screen readers.
    pub labelled_by: Option<Id>,
}

/// Draws the field; returns the number to write when a gesture ended on a changed value.
pub fn show(ui: &mut Ui, field: &NumberField) -> Option<f64> {
    let pending_id = field.id.with(PENDING_NUMBER_ID);
    let held = ui.data(|data| data.get_temp::<f64>(pending_id));
    // Before the first report the field starts at the range's low end (or 0 without one).
    let low = Some(*field.range.start()).filter(|low| low.is_finite());
    let mut value = held.or(field.device).or(low).unwrap_or_default();
    let response = drag_value(ui, field, &mut value);
    let device = field.device;
    if response.dragged() {
        let start = device.unwrap_or(value);
        value = bounded_move(start, value, &field.range).unwrap_or(start);
    }
    let active = response.dragged() || response.has_focus();
    let ended = response.drag_stopped() || response.lost_focus();
    if !active && !ended {
        ui.data_mut(|data| data.remove::<f64>(pending_id));
        return None;
    }
    // egui also reports a change on the frame focus leaves, when it reparses and clamps the
    // shown text; that one is the user's only if the same frame carried their typing.
    let typed_this_frame = ui.input(|input| input.events.iter().any(is_text_edit));
    let changed_by_user = response.changed() && (active || typed_this_frame);
    if changed_by_user {
        ui.data_mut(|data| data.insert_temp(pending_id, value));
    }
    if active {
        return None;
    }
    let number = changed_by_user.then_some(value).or(held)?;
    ui.data_mut(|data| data.remove::<f64>(pending_id));
    if ui.input(|input| input.key_pressed(Key::Escape)) {
        return None;
    }
    // A drag or an edit that ends where it began writes nothing.
    let unchanged = match (field.step, device) {
        (Some(step), Some(device)) => released(device, number, step).is_none(),
        _ => Some(number) == device,
    };
    (!unchanged).then_some(number)
}

/// No range on the egui field: it would clamp typed text to it, and a typo would reach the
/// speaker as the range's end. The field shows the value unrounded: egui parses the shown text
/// when focus leaves and would otherwise report the rounded text as an edit.
fn drag_value(ui: &mut Ui, field: &NumberField, value: &mut f64) -> Response {
    let mut drag = DragValue::new(value)
        .custom_formatter(|number, _| texts::number(number))
        .custom_parser(|text| typed_value(text, None))
        .speed(field.drag_speed);
    // A converted field shows the converted unit only; both would read "0 1/48 kHz ms".
    let device_unit = field.unit.filter(|_| field.conversion.is_none());
    if let Some(unit) = device_unit {
        drag = drag.suffix(format!(" {unit}"));
    }
    if let Some(conversion) = field.conversion {
        // Typed and shown in the converted unit; back on the device's step grid, so the shown
        // text parses to exactly the device value and a visit writes nothing.
        let step = field.drag_speed;
        drag = drag
            .custom_formatter(move |number, _| texts::number(conversion.apply(number)))
            .custom_parser(move |text| {
                typed_value(text, Some(conversion)).map(|typed| on_grid(typed, step))
            })
            .suffix(format!(" {}", converted_unit(conversion)));
    }
    let response = ui.add_enabled(field.enabled, drag);
    match field.labelled_by {
        Some(label) => response.labelled_by(label),
        None => response,
    }
}
