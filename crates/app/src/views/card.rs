//! One speaker as a channel strip: name, status, level fader with input meters, mute and
//! identify, the "Mehr" toggle and a status line. The card only reports what was clicked;
//! `writes` decides whether it becomes a command.

use eframe::egui::{
    self, pos2, vec2, Button, CornerRadius, Label, Rect, RichText, Sense, Stroke, Ui, Vec2,
};
use neumann_control_core::catalog::Catalog;
use neumann_control_core::mask::mask_serial;
use neumann_control_core::speaker::Command;
use serde_json::Value;

use super::fader::{self, FADER_HEIGHT};
use crate::fader::output_level_range;
use crate::meter::fraction;
use crate::paths;
use crate::state::{SpeakerState, Status};
use crate::texts;
use crate::ui_kit::theme::Tokens;
use crate::writes::{self, Intent, Switch};

const CARD_WIDTH: f32 = 170.0;
const CARD_MARGIN: i8 = 8;
const CARD_RADIUS: u8 = 6;
const BORDER_WIDTH: f32 = 1.0;
const METER_WIDTH: f32 = 8.0;
const CLIP_LAMP_RADIUS: f32 = 3.5;
const CLIP_LAMP_SPACE: f32 = 10.0;
const PEAK_LINE_WIDTH: f32 = 2.0;
const STATUS_DOT_RADIUS: f32 = 4.0;
const UNREACHABLE_OPACITY: f32 = 0.55;

/// Draws the card and returns the writes the user asked for that the write policy allowed.
pub fn show(ui: &mut Ui, speaker: &mut SpeakerState, catalog: &Catalog) -> Vec<Command> {
    let tokens = Tokens::current(ui.ctx());
    let mut commands = Vec::new();
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
            strip(ui, speaker, catalog, &mut commands, &tokens);
            buttons(ui, speaker, catalog, &mut commands);
            more_toggle(ui, speaker);
            status_line(ui, speaker, &tokens);
        });
    commands
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

fn strip(
    ui: &mut Ui,
    speaker: &mut SpeakerState,
    catalog: &Catalog,
    commands: &mut Vec<Command>,
    tokens: &Tokens,
) {
    let path = paths::path(paths::OUTPUT_LEVEL);
    let device = speaker.values.get(&path).and_then(Value::as_f64);
    let shown = ui
        .horizontal(|ui| {
            let shown = fader::show(ui, speaker, catalog, commands, device, output_level_range());
            meters(ui, speaker, tokens);
            shown
        })
        .inner;
    let unit = paths::catalog_entry(catalog, &path)
        .and_then(|entry| entry.unit.clone())
        .unwrap_or_default();
    let readout = device.map(|_| texts::readout(shown, &unit));
    ui.label(readout.unwrap_or_default());
}

fn meters(ui: &mut Ui, speaker: &SpeakerState, tokens: &Tokens) {
    let now = ui.input(|input| input.time);
    let clips = speaker
        .values
        .get(&paths::path(paths::INPUT_CLIP))
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

fn buttons(
    ui: &mut Ui,
    speaker: &mut SpeakerState,
    catalog: &Catalog,
    commands: &mut Vec<Command>,
) {
    ui.horizontal(|ui| {
        toggle(ui, speaker, catalog, commands, Switch::Mute, texts::MUTE);
        toggle(
            ui,
            speaker,
            catalog,
            commands,
            Switch::Identify,
            texts::IDENTIFY,
        );
    });
}

/// A button showing a switch value of the speaker; a click asks for the opposite.
fn toggle(
    ui: &mut Ui,
    speaker: &mut SpeakerState,
    catalog: &Catalog,
    commands: &mut Vec<Command>,
    switch: Switch,
    text: &str,
) {
    let path = switch.path();
    let current = speaker.values.get(&path).and_then(Value::as_bool);
    let enabled = current.is_some() && writes::can_write(speaker, catalog, &path);
    let button = Button::new(text).selected(current.unwrap_or(false));
    if ui.add_enabled(enabled, button).clicked() {
        commands.extend(speaker.request(Intent::Toggle(switch), catalog));
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
