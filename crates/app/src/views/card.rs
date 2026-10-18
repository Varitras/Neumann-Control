//! One speaker as a channel strip: name, status, level fader with input meters, mute and
//! identify, the "Mehr" toggle with the settings it opens and "Entfernen", and a status line.
//! The card only reports what was clicked; `writes` decides whether it becomes a command.

use eframe::egui::{self, Button, CornerRadius, Label, RichText, Sense, Stroke, Ui, Vec2};
use neumann_control_core::catalog::Catalog;
use neumann_control_core::mask::mask_serial;
use neumann_control_core::speaker::Command;
use serde_json::Value;

use super::fader::{self, FADER_HEIGHT};
use super::{details, meters};
use crate::display::firmware_version;
use crate::fader::output_level_range;
use crate::paths;
use crate::state::{SpeakerState, Status};
use crate::texts;
use crate::ui_kit::theme::Tokens;
use crate::writes::{self, Intent, Switch};

const CARD_WIDTH: f32 = 170.0;
const CARD_MARGIN: i8 = 8;
const CARD_RADIUS: u8 = 6;
const BORDER_WIDTH: f32 = 1.0;
const STATUS_DOT_RADIUS: f32 = 4.0;
const UNREACHABLE_OPACITY: f32 = 0.55;
const DETAILS_ID: &str = "details";

pub struct CardResponse {
    /// The writes the user asked for that the write policy allowed.
    pub commands: Vec<Command>,
    pub remove_clicked: bool,
}

pub fn show(ui: &mut Ui, speaker: &mut SpeakerState, catalog: &Catalog) -> CardResponse {
    let tokens = Tokens::current(ui.ctx());
    let mut commands = Vec::new();
    // The parent lays cards out in a wrapping row; the content must not inherit that flow.
    let remove_clicked = ui
        .vertical(|ui| card_frame(ui, speaker, catalog, &mut commands, &tokens))
        .inner;
    CardResponse {
        commands,
        remove_clicked,
    }
}

/// True when "Entfernen" was clicked.
fn card_frame(
    ui: &mut Ui,
    speaker: &mut SpeakerState,
    catalog: &Catalog,
    commands: &mut Vec<Command>,
    tokens: &Tokens,
) -> bool {
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
            header(ui, speaker, tokens);
            strip(ui, speaker, catalog, commands, tokens);
            buttons(ui, speaker, catalog, commands);
            more_toggle(ui, speaker);
            // The scope exists collapsed too, so opening it never renumbers the widgets after it.
            let expanded = ui.push_id(DETAILS_ID, |ui| {
                if !speaker.expanded {
                    return (Vec::new(), false);
                }
                let commands = details::show(ui, speaker, catalog);
                (commands, ui.button(texts::REMOVE).clicked())
            });
            let (expanded_commands, remove_clicked) = expanded.inner;
            commands.extend(expanded_commands);
            status_line(ui, speaker, tokens);
            remove_clicked
        })
        .inner
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
    let clips = speaker.values.get(&paths::path(paths::INPUT_CLIP));
    meters::show(ui, &speaker.input_meters, clips, FADER_HEIGHT, tokens);
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
        let now = ui.input(|input| input.time);
        commands.extend(speaker.request(Intent::Toggle(switch), catalog, now));
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
        let line = texts::status_line(
            &firmware_version(&identity.version),
            &mask_serial(&identity.serial),
        );
        ui.add(Label::new(RichText::new(line).small().weak()).wrap());
    }
    if let Some(message) = &speaker.message {
        ui.add(Label::new(RichText::new(message).small().color(tokens.danger)).wrap());
    }
}
