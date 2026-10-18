//! The level fader, upright on a strip and lying in a compact row: dragged relative to the
//! pointer (Shift for fine moves), written once when let go, and the mouse wheel in 0.5 dB steps
//! per notch.

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

/// The upright fader's length; the whole range spans it.
pub const FADER_HEIGHT: f32 = 200.0;
/// The lying fader's length in a compact row.
const BAR_LENGTH: f32 = 160.0;
/// Space between a lying fader and the value shown above its handle while dragging.
const VALUE_ABOVE_GAP: f32 = 4.0;

/// Which way a fader runs: upright on a strip, lying in a compact row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    Vertical,
    Horizontal,
}

impl Orientation {
    fn size(self) -> Vec2 {
        match self {
            Orientation::Vertical => vec2(FADER_WIDTH, FADER_HEIGHT),
            Orientation::Horizontal => vec2(BAR_LENGTH, FADER_WIDTH),
        }
    }

    fn length(self) -> f32 {
        match self {
            Orientation::Vertical => FADER_HEIGHT,
            Orientation::Horizontal => BAR_LENGTH,
        }
    }

    /// The part of a drag along the fader, towards higher values positive.
    fn along(self, delta: Vec2) -> f64 {
        match self {
            Orientation::Vertical => -f64::from(delta.y),
            Orientation::Horizontal => f64::from(delta.x),
        }
    }

    fn cursor(self) -> CursorIcon {
        match self {
            Orientation::Vertical => CursorIcon::ResizeVertical,
            Orientation::Horizontal => CursorIcon::ResizeHorizontal,
        }
    }
}

/// What a fader spans and how it lies.
pub struct Scale<'a> {
    pub range: RangeInclusive<f64>,
    pub unit: &'a str,
    pub orientation: Orientation,
}

/// A speaker's level fader; returns the value it shows. It moves relative to the pointer from
/// wherever it is grabbed, never jumping to the pointer; Shift makes it `FINE_DIVISOR` times
/// finer. While dragging, the value also stands beside the fader, where the eye is.
pub fn show(
    ui: &mut Ui,
    speaker: &mut SpeakerState,
    catalog: &Catalog,
    commands: &mut Vec<Command>,
    device: Option<f64>,
    scale: Scale,
) -> f64 {
    let Scale {
        range,
        unit,
        orientation,
    } = scale;
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
    let (rect, response) = ui.allocate_exact_size(orientation.size(), sense);
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
        let points = orientation.along(response.drag_delta());
        let moved = drag_change(&range, points, fine, orientation.length());
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
        ui.ctx().set_cursor_icon(orientation.cursor());
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
    paint(ui, rect, &range, value, enabled, orientation, &tokens);
    response.widget_info(|| WidgetInfo::slider(enabled, value, texts::LEVEL));
    let id = response.id;
    response.on_hover_text(texts::CONTROL_HINT);
    if dragging {
        let text = texts::readout(value, unit);
        dragged_value(ui, id, rect, &range, value, orientation, &text);
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

/// A fader for several speakers at once (group, master): it shows `reference` and returns the
/// value it was let go at; the caller turns the move into one write per speaker. No wheel, so a
/// group move is always one deliberate gesture.
pub fn shared(ui: &mut Ui, label: &str, reference: Option<f64>, scale: Scale) -> Option<f64> {
    let Scale {
        range,
        unit,
        orientation,
    } = scale;
    let tokens = Tokens::current(ui.ctx());
    let enabled = reference.is_some();
    let mut value = reference.unwrap_or(*range.start());
    let sense = if enabled {
        Sense::drag()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(orientation.size(), sense);
    let drag_id = response.id.with(DRAG_VALUE_ID);
    if response.drag_started() {
        ui.data_mut(|data| data.insert_temp(drag_id, value));
    }
    let aborted = drag_aborted(ui, &response);
    let dragging = response.dragged() && !aborted;
    if dragging {
        let held = ui
            .data(|data| data.get_temp::<f64>(drag_id))
            .unwrap_or(value);
        let fine = ui.input(|input| input.modifiers.shift);
        let points = orientation.along(response.drag_delta());
        let moved = drag_change(&range, points, fine, orientation.length());
        let unsnapped = (held + moved).clamp(*range.start(), *range.end());
        ui.data_mut(|data| data.insert_temp(drag_id, unsnapped));
        value = on_level_grid(unsnapped);
    }
    let mut released = None;
    if response.drag_stopped() {
        let dragged = ui.data(|data| data.get_temp::<f64>(drag_id));
        ui.data_mut(|data| data.remove::<f64>(drag_id));
        released = dragged
            .filter(|_| !aborted)
            .map(on_level_grid)
            .filter(|released| Some(*released) != reference);
    }
    if enabled && response.hovered() {
        ui.ctx().set_cursor_icon(orientation.cursor());
    }
    paint(ui, rect, &range, value, enabled, orientation, &tokens);
    response.widget_info(|| WidgetInfo::slider(enabled, value, label));
    let id = response.id;
    response.on_hover_text(texts::CONTROL_HINT);
    if dragging {
        let text = texts::readout(value, unit);
        dragged_value(ui, id, rect, &range, value, orientation, &text);
    }
    released
}

/// The value change for a drag of `points` along a fader of `length`, which spans the whole
/// range.
fn drag_change(range: &RangeInclusive<f64>, points: f64, fine: bool, length: f32) -> f64 {
    let span = range.end() - range.start();
    let divisor = if fine { FINE_DIVISOR } else { 1.0 };
    points / f64::from(length) * span / divisor
}

fn on_level_grid(value: f64) -> f64 {
    (value / WHEEL_STEP_DB).round() * WHEEL_STEP_DB
}

/// Where `value` sits along the fader: a y on an upright one, an x on a lying one.
fn value_position(
    rect: Rect,
    range: &RangeInclusive<f64>,
    value: f64,
    orientation: Orientation,
) -> f32 {
    let span = range.end() - range.start();
    let position = ((value - range.start()) / span).clamp(0.0, 1.0) as f32;
    match orientation {
        Orientation::Vertical => rect.bottom() - position * rect.height(),
        Orientation::Horizontal => rect.left() + position * rect.width(),
    }
}

fn paint(
    ui: &Ui,
    rect: Rect,
    range: &RangeInclusive<f64>,
    value: f64,
    enabled: bool,
    orientation: Orientation,
    tokens: &Tokens,
) {
    let painter = ui.painter();
    let at = value_position(rect, range, value, orientation);
    let color = if enabled {
        tokens.accent
    } else {
        tokens.text_dim
    };
    let (track, fill, cap) = match orientation {
        Orientation::Vertical => {
            let track = Rect::from_center_size(rect.center(), vec2(TRACK_WIDTH, rect.height()));
            let fill = Rect::from_min_max(egui::pos2(track.left(), at), track.right_bottom());
            let center = egui::pos2(rect.center().x, at);
            (
                track,
                fill,
                Rect::from_center_size(center, vec2(CAP_WIDTH, CAP_HEIGHT)),
            )
        }
        Orientation::Horizontal => {
            let track = Rect::from_center_size(rect.center(), vec2(rect.width(), TRACK_WIDTH));
            let fill = Rect::from_min_max(track.left_top(), egui::pos2(at, track.bottom()));
            let center = egui::pos2(at, rect.center().y);
            (
                track,
                fill,
                Rect::from_center_size(center, vec2(CAP_HEIGHT, CAP_WIDTH)),
            )
        }
    };
    painter.rect_filled(track, CornerRadius::same(TRACK_RADIUS), tokens.meter_track);
    painter.rect_filled(fill, CornerRadius::same(TRACK_RADIUS), color);
    painter.rect(
        cap,
        CornerRadius::same(CAP_RADIUS),
        tokens.field,
        Stroke::new(CAP_BORDER_WIDTH, tokens.border),
        StrokeKind::Inside,
    );
    let line = Stroke::new(CAP_LINE_WIDTH, tokens.text);
    match orientation {
        Orientation::Vertical => painter.hline(cap.x_range(), at, line),
        Orientation::Horizontal => painter.vline(at, cap.y_range(), line),
    };
}

/// The value while dragging, by the handle: beside an upright fader, above a lying one, so the
/// pointer never covers it.
fn dragged_value(
    ui: &Ui,
    id: Id,
    rect: Rect,
    range: &RangeInclusive<f64>,
    value: f64,
    orientation: Orientation,
    text: &str,
) {
    let at = value_position(rect, range, value, orientation);
    match orientation {
        Orientation::Vertical => value_beside(ui, id, rect, at, text),
        Orientation::Horizontal => {
            let anchor = egui::pos2(at, rect.top() - VALUE_ABOVE_GAP);
            floating_value(ui, id, anchor, Align2::CENTER_BOTTOM, text);
        }
    }
}

/// A value floating right of `rect` at height `y` while a control is being moved, where the
/// eye is; fader and knob both use it.
pub(super) fn value_beside(ui: &Ui, id: Id, rect: Rect, y: f32, text: &str) {
    let anchor = egui::pos2(rect.right() + HANDLE_LABEL_GAP, y);
    floating_value(ui, id, anchor, Align2::LEFT_CENTER, text);
}

fn floating_value(ui: &Ui, id: Id, anchor: egui::Pos2, pivot: Align2, text: &str) {
    Area::new(id.with(HANDLE_LABEL_ID))
        .order(Order::Tooltip)
        .fixed_pos(anchor)
        .pivot(pivot)
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
