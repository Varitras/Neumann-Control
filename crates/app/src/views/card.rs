//! One speaker as a channel strip: name, status, level fader with input meters, mute and
//! identify, the "Mehr" toggle and a status line.

use std::ops::RangeInclusive;

use eframe::egui::{
    self, pos2, vec2, Button, CornerRadius, Event, Label, Rect, RichText, Sense, Slider, Stroke,
    Ui, Vec2,
};
use neumann_control_core::catalog::{Catalog, Entry};
use neumann_control_core::mask::mask_serial;
use neumann_control_core::speaker::Command;
use neumann_control_core::ssc::wire::SscPath;
use serde_json::Value;

use crate::state::{
    fraction, output_level_range, SpeakerState, Status, OUTPUT_LEVEL_PATH, WHEEL_STEP_DB,
};
use crate::texts;
use crate::ui_kit::theme::Tokens;

const CARD_WIDTH: f32 = 170.0;
const CARD_MARGIN: i8 = 8;
const CARD_RADIUS: u8 = 6;
const BORDER_WIDTH: f32 = 1.0;
const FADER_HEIGHT: f32 = 150.0;
const METER_WIDTH: f32 = 8.0;
const CLIP_LAMP_RADIUS: f32 = 3.5;
const CLIP_LAMP_SPACE: f32 = 10.0;
const PEAK_LINE_WIDTH: f32 = 2.0;
const STATUS_DOT_RADIUS: f32 = 4.0;
const UNREACHABLE_OPACITY: f32 = 0.55;

const MUTE_PATH: &str = "audio/out/mute";
const IDENTIFY_PATH: &str = "device/identification/visual";
const INPUT_CLIP_PATH: &str = "m/in/clip";

/// Draws the card and returns the writes the user asked for, already checked against the
/// catalog for this model.
pub fn show(ui: &mut Ui, speaker: &mut SpeakerState, catalog: &Catalog) -> Vec<Command> {
    let tokens = Tokens::current(ui.ctx());
    let mut writer = Writer {
        catalog,
        product: speaker.model().unwrap_or_default().to_owned(),
        connected: speaker.status == Status::Connected,
        commands: Vec::new(),
    };
    let inner_width = CARD_WIDTH - 2.0 * (f32::from(CARD_MARGIN) + BORDER_WIDTH);
    egui::Frame::new()
        .fill(tokens.card)
        .stroke(Stroke::new(BORDER_WIDTH, tokens.border))
        .corner_radius(CornerRadius::same(CARD_RADIUS))
        .inner_margin(CARD_MARGIN)
        .show(ui, |ui| {
            ui.set_width(inner_width);
            if speaker.status == Status::Unreachable {
                ui.multiply_opacity(UNREACHABLE_OPACITY);
            }
            header(ui, speaker, &tokens);
            strip(ui, speaker, &mut writer, &tokens);
            buttons(ui, speaker, &mut writer);
            more_toggle(ui, speaker);
            status_line(ui, speaker, &tokens);
        });
    writer.commands
}

/// Collects the card's writes; a write goes out only to a connected speaker of a model the
/// catalog lists as writable for the path, with a value the catalog accepts.
struct Writer<'a> {
    catalog: &'a Catalog,
    product: String,
    connected: bool,
    commands: Vec<Command>,
}

impl Writer<'_> {
    fn entry(&self, path: &SscPath) -> Option<&Entry> {
        self.catalog
            .entries
            .iter()
            .find(|entry| entry.path == *path)
    }

    fn can_write(&self, path: &SscPath) -> bool {
        let writable = self
            .entry(path)
            .is_some_and(|entry| entry.is_writable(&self.product));
        self.connected && writable
    }

    fn set(&mut self, path: SscPath, value: Value) {
        if !self.can_write(&path) {
            return;
        }
        let Some(entry) = self.entry(&path) else {
            return;
        };
        if let Err(error) = entry.validate(&value, &self.product, &self.catalog.classes) {
            log::warn!("not sending {path}: {error}");
            return;
        }
        self.commands.push(Command::Set { path, value });
    }
}

fn constant_path(text: &str) -> SscPath {
    SscPath::parse(text).expect("constant path is valid")
}

fn header(ui: &mut Ui, speaker: &SpeakerState, tokens: &Tokens) {
    let name = speaker.display_name();
    ui.horizontal(|ui| {
        status_dot(ui, speaker.status, tokens);
        ui.add(Label::new(RichText::new(&name).strong()).truncate());
    });
    let model = speaker.model().filter(|model| *model != name);
    if let Some(model) = model {
        ui.add(Label::new(RichText::new(model).weak()).truncate());
    }
}

fn status_dot(ui: &mut Ui, status: Status, tokens: &Tokens) {
    let (color, text) = match status {
        Status::Connected => (tokens.connected, texts::CONNECTED),
        Status::Connecting => (tokens.connecting, texts::CONNECTING),
        Status::Unreachable => (tokens.unreachable, texts::UNREACHABLE),
    };
    let size = Vec2::splat(2.0 * STATUS_DOT_RADIUS);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter()
        .circle_filled(rect.center(), STATUS_DOT_RADIUS, color);
    response.on_hover_text(text);
}

fn strip(ui: &mut Ui, speaker: &mut SpeakerState, writer: &mut Writer, tokens: &Tokens) {
    let path = constant_path(OUTPUT_LEVEL_PATH);
    let device = speaker.values.get(&path).and_then(Value::as_f64);
    let range = output_level_range();
    let shown = ui
        .horizontal(|ui| {
            let shown = fader(ui, speaker, writer, device, range);
            meters(ui, speaker, tokens);
            shown
        })
        .inner;
    let unit = writer
        .entry(&path)
        .and_then(|entry| entry.unit.clone())
        .unwrap_or_default();
    let readout = device.map(|_| texts::readout(shown, &unit));
    ui.label(readout.unwrap_or_default());
}

/// The vertical level fader; returns the value it shows.
fn fader(
    ui: &mut Ui,
    speaker: &mut SpeakerState,
    writer: &mut Writer,
    device: Option<f64>,
    range: RangeInclusive<f64>,
) -> f64 {
    let path = constant_path(OUTPUT_LEVEL_PATH);
    let enabled = device.is_some() && writer.can_write(&path);
    let device = device.unwrap_or(*range.start());
    let now = ui.input(|input| input.time);
    let mut value = speaker.fader.shown(device);
    ui.spacing_mut().slider_width = FADER_HEIGHT;
    let slider = Slider::new(&mut value, range.clone())
        .vertical()
        .show_value(false)
        .step_by(WHEEL_STEP_DB);
    let response = ui.add_enabled(enabled, slider);
    // While dragging, the slider follows the pointer and the speaker hears nothing. The release
    // frame still moves the value but no longer counts as dragged, so this one condition sends
    // the final value of a drag, a click on the track and a key press alike.
    let settled = response.changed() && !response.dragged();
    if settled {
        writer.set(path.clone(), Value::from(value));
    }
    if enabled && response.hovered() {
        let steps = wheel_steps(ui);
        // The cards sit in a scroll area, which would scroll along with the fader.
        ui.input_mut(|input| input.smooth_scroll_delta = Vec2::ZERO);
        if steps != 0 {
            speaker.fader.on_wheel(steps, device, range, now);
            value = speaker.fader.shown(device);
        }
    }
    if let Some(due) = speaker.fader.due(now) {
        writer.set(path, Value::from(due));
    }
    if let Some(deadline) = speaker.fader.deadline() {
        let wait = (deadline - now).max(0.0);
        ui.ctx().request_repaint_after_secs(wait as f32);
    }
    value
}

/// One step per wheel event, upwards raising the level, whatever unit the device reports.
fn wheel_steps(ui: &Ui) -> i32 {
    ui.input(|input| {
        input
            .events
            .iter()
            .filter_map(|event| match event {
                Event::MouseWheel { delta, .. } if delta.y != 0.0 => Some(delta.y.signum() as i32),
                _ => None,
            })
            .sum()
    })
}

fn meters(ui: &mut Ui, speaker: &SpeakerState, tokens: &Tokens) {
    let now = ui.input(|input| input.time);
    let clips = speaker
        .values
        .get(&constant_path(INPUT_CLIP_PATH))
        .and_then(Value::as_array);
    for (channel, meter) in speaker.input_meters.iter().enumerate() {
        let clipped = clips
            .and_then(|clips| clips.get(channel))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let size = vec2(METER_WIDTH, FADER_HEIGHT);
        let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
        let lamp_color = if clipped {
            tokens.danger
        } else {
            tokens.clip_off
        };
        let lamp_center = pos2(rect.center().x, rect.top() + CLIP_LAMP_RADIUS);
        let track = Rect::from_min_max(pos2(rect.left(), rect.top() + CLIP_LAMP_SPACE), rect.max);
        let painter = ui.painter();
        painter.circle_filled(lamp_center, CLIP_LAMP_RADIUS, lamp_color);
        painter.rect_filled(track, CornerRadius::ZERO, tokens.meter_track);
        let level_top = track.bottom() - track.height() * fraction(meter.level());
        let level = Rect::from_min_max(pos2(track.left(), level_top), track.max);
        painter.rect_filled(level, CornerRadius::ZERO, tokens.meter_fill);
        let peak_y = track.bottom() - track.height() * fraction(meter.peak(now));
        painter.hline(
            track.x_range(),
            peak_y,
            Stroke::new(PEAK_LINE_WIDTH, tokens.text_dim),
        );
        if clipped {
            response.on_hover_text(texts::CLIP);
        }
    }
}

fn buttons(ui: &mut Ui, speaker: &SpeakerState, writer: &mut Writer) {
    ui.horizontal(|ui| {
        toggle(ui, speaker, writer, MUTE_PATH, texts::MUTE);
        toggle(ui, speaker, writer, IDENTIFY_PATH, texts::IDENTIFY);
    });
}

/// A button showing a switch value of the speaker; a click writes the opposite.
fn toggle(ui: &mut Ui, speaker: &SpeakerState, writer: &mut Writer, path: &str, text: &str) {
    let path = constant_path(path);
    let current = speaker.values.get(&path).and_then(Value::as_bool);
    let enabled = current.is_some() && writer.can_write(&path);
    let on = current.unwrap_or(false);
    let clicked = ui
        .add_enabled(enabled, Button::new(text).selected(on))
        .clicked();
    if clicked {
        writer.set(path, Value::Bool(!on));
    }
}

fn more_toggle(ui: &mut Ui, speaker: &mut SpeakerState) {
    let text = if speaker.expanded {
        texts::LESS
    } else {
        texts::MORE
    };
    if ui.button(text).clicked() {
        speaker.expanded = !speaker.expanded;
    }
}

fn status_line(ui: &mut Ui, speaker: &SpeakerState, tokens: &Tokens) {
    if let Some(identity) = &speaker.identity {
        let line = texts::status_line(&identity.version, &mask_serial(&identity.serial));
        ui.add(Label::new(RichText::new(line).small().weak()).wrap());
    }
    if let Some(message) = &speaker.message {
        ui.add(Label::new(RichText::new(message).small().color(tokens.danger)).wrap());
    }
}
