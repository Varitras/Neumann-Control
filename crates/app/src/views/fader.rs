//! The strip's level fader: dragged relative to the pointer (Shift for fine moves), written once
//! when let go, and the mouse wheel in 0.5 dB steps per notch.

use std::ops::RangeInclusive;

use eframe::egui::{
    self, vec2, Align2, Area, CornerRadius, CursorIcon, Event, Frame, Id, Key, Label,
    MouseWheelUnit, Order, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo,
};
use neumann_control_core::catalog::Catalog;
use neumann_control_core::speaker::Command;

use crate::fader::WHEEL_STEP_DB;
use crate::paths;
use crate::state::SpeakerState;
use crate::texts;
use crate::ui_kit::theme::Tokens;
use crate::writes::{self, Intent};

const DRAG_ABORTED_ID: &str = "fader_drag_aborted";
const HANDLE_LABEL_ID: &str = "fader_handle_label";
const DRAG_VALUE_ID: &str = "fader_drag_value";
const FADER_WIDTH: f32 = 24.0;
const TRACK_WIDTH: f32 = 4.0;
const TRACK_RADIUS: u8 = 2;
const CAP_WIDTH: f32 = 22.0;
const CAP_HEIGHT: f32 = 12.0;
const CAP_RADIUS: u8 = 3;
const CAP_BORDER_WIDTH: f32 = 1.0;
const CAP_LINE_WIDTH: f32 = 1.5;
/// Shift makes a drag this many times finer, on the fader and on knobs.
pub(super) const FINE_DIVISOR: f64 = 10.0;
/// Space between the fader and the value shown beside its handle while dragging.
const HANDLE_LABEL_GAP: f32 = 6.0;

/// The fader's length; the whole range spans it.
pub const FADER_HEIGHT: f32 = 200.0;

/// The vertical level fader; returns the value it shows. It moves relative to the pointer from
/// wherever it is grabbed, never jumping to the pointer; Shift makes it `FINE_DIVISOR` times
/// finer. While dragging, the value also stands beside the fader, where the eye is.
pub fn show(
    ui: &mut Ui,
    speaker: &mut SpeakerState,
    catalog: &Catalog,
    commands: &mut Vec<Command>,
    device: Option<f64>,
    range: RangeInclusive<f64>,
    unit: &str,
) -> f64 {
    let tokens = Tokens::current(ui.ctx());
    let path = paths::path(paths::OUTPUT_LEVEL);
    let enabled = device.is_some() && writes::can_write(speaker, catalog, &path);
    let device = device.unwrap_or(*range.start());
    let now = ui.input(|input| input.time);
    let mut value = speaker.fader.shown(device, now);
    // Drag only: a drag then starts on the press itself, so a wheel step pending from just
    // before cannot fall due in between and reach the speaker.
    let sense = if enabled {
        Sense::drag()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(FADER_WIDTH, FADER_HEIGHT), sense);
    let drag_id = response.id.with(DRAG_VALUE_ID);
    if response.drag_started() {
        speaker.fader.drag_started();
        ui.data_mut(|data| data.insert_temp(drag_id, value));
    }
    let aborted = drag_aborted(ui, &response);
    let dragging = response.dragged() && !aborted;
    if dragging {
        let held = ui
            .data(|data| data.get_temp::<f64>(drag_id))
            .unwrap_or(value);
        let fine = ui.input(|input| input.modifiers.shift);
        let moved = drag_change(&range, -f64::from(response.drag_delta().y), fine);
        let unsnapped = (held + moved).clamp(*range.start(), *range.end());
        ui.data_mut(|data| data.insert_temp(drag_id, unsnapped));
        value = on_level_grid(unsnapped);
    }
    // The speaker hears nothing while dragging; the release sends the value once.
    if response.drag_stopped() {
        let dragged = ui.data(|data| data.get_temp::<f64>(drag_id));
        ui.data_mut(|data| data.remove::<f64>(drag_id));
        if let Some(dragged) = dragged.filter(|_| !aborted) {
            value = on_level_grid(dragged);
            commands.extend(speaker.request(Intent::Level(value), catalog, now));
        }
    }
    // A held fader ignores the wheel: steps added now would overwrite the drag's value.
    let held = response.is_pointer_button_down_on() || response.dragged();
    if enabled && response.hovered() && !held {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
        // The strips sit in a scroll area, which would scroll along with the fader.
        ui.input_mut(|input| input.smooth_scroll_delta = Vec2::ZERO);
        let wheel = wheel_notches(ui);
        for notches in &wheel {
            speaker
                .fader
                .on_wheel(*notches, WHEEL_STEP_DB, device, range.clone(), now);
        }
        if !wheel.is_empty() {
            value = speaker.fader.shown(device, now);
        }
    }
    paint(ui, rect, &range, value, enabled, &tokens);
    response.widget_info(|| WidgetInfo::slider(enabled, value, texts::LEVEL));
    let id = response.id;
    response.on_hover_text(texts::CONTROL_HINT);
    if dragging {
        let y = value_y(rect, &range, value);
        value_beside(ui, id, rect, y, &texts::readout(value, unit));
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

/// The value change for a drag of `points` (upwards positive): the fader's length is the
/// whole range.
pub fn drag_change(range: &RangeInclusive<f64>, points: f64, fine: bool) -> f64 {
    let span = range.end() - range.start();
    let divisor = if fine { FINE_DIVISOR } else { 1.0 };
    points / f64::from(FADER_HEIGHT) * span / divisor
}

fn on_level_grid(value: f64) -> f64 {
    (value / WHEEL_STEP_DB).round() * WHEEL_STEP_DB
}

fn value_y(rect: Rect, range: &RangeInclusive<f64>, value: f64) -> f32 {
    let span = range.end() - range.start();
    let position = ((value - range.start()) / span).clamp(0.0, 1.0) as f32;
    rect.bottom() - position * rect.height()
}

fn paint(
    ui: &Ui,
    rect: Rect,
    range: &RangeInclusive<f64>,
    value: f64,
    enabled: bool,
    tokens: &Tokens,
) {
    let painter = ui.painter();
    let track = Rect::from_center_size(rect.center(), vec2(TRACK_WIDTH, rect.height()));
    painter.rect_filled(track, CornerRadius::same(TRACK_RADIUS), tokens.meter_track);
    let y = value_y(rect, range, value);
    let fill = Rect::from_min_max(egui::pos2(track.left(), y), track.right_bottom());
    let color = if enabled {
        tokens.accent
    } else {
        tokens.text_dim
    };
    painter.rect_filled(fill, CornerRadius::same(TRACK_RADIUS), color);
    let cap = Rect::from_center_size(egui::pos2(rect.center().x, y), vec2(CAP_WIDTH, CAP_HEIGHT));
    painter.rect(
        cap,
        CornerRadius::same(CAP_RADIUS),
        tokens.field,
        Stroke::new(CAP_BORDER_WIDTH, tokens.border),
        StrokeKind::Inside,
    );
    painter.hline(cap.x_range(), y, Stroke::new(CAP_LINE_WIDTH, tokens.text));
}

/// A value floating right of `rect` at height `y` while a control is being moved, where the
/// eye is; fader and knob both use it.
pub(super) fn value_beside(ui: &Ui, id: Id, rect: Rect, y: f32, text: &str) {
    let anchor = egui::pos2(rect.right() + HANDLE_LABEL_GAP, y);
    Area::new(id.with(HANDLE_LABEL_ID))
        .order(Order::Tooltip)
        .fixed_pos(anchor)
        .pivot(Align2::LEFT_CENTER)
        .interactable(false)
        .show(ui.ctx(), |ui| {
            // One line: the area is narrow and would wrap the value letter by letter.
            Frame::popup(ui.style()).show(ui, |ui| ui.add(Label::new(text).extend()));
        });
}

/// Escape ends egui's drag but leaves the button down, and the slider keeps following the
/// pointer until it is released; none of that may reach the speaker. True from the Escape
/// frame through the release frame.
pub(super) fn drag_aborted(ui: &Ui, response: &Response) -> bool {
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
pub(super) fn wheel_notches(ui: &Ui) -> Vec<f64> {
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
