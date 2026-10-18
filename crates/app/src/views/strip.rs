//! One speaker as a channel strip: name (opens the device panel), status LEDs, delay and
//! standby knobs, level fader with meters, readout, response time, mute and identify, and the
//! last message. The strip only reports gestures; `writes` decides what becomes a command.

use eframe::egui::{
    self, Button, Color32, CornerRadius, Label, Pos2, RichText, Sense, Stroke, Ui, Vec2,
    WidgetInfo, WidgetType,
};
use neumann_control_core::catalog::Catalog;
use neumann_control_core::settings::DeskSettings;
use neumann_control_core::speaker::Command;
use serde_json::Value;

use super::fader::{self, FADER_HEIGHT};
use super::{knob, meters};
use crate::fader::output_level_range;
use crate::paths;
use crate::state::{SpeakerState, Status};
use crate::status_leds::{status_leds, ConnectionLed};
use crate::texts;
use crate::ui_kit::theme::Tokens;
use crate::writes::{self, Intent, Switch};

pub const STRIP_WIDTH: f32 = 112.0;
const STRIP_MARGIN: i8 = 6;
const STRIP_RADIUS: u8 = 6;
const BORDER_WIDTH: f32 = 1.0;
const LED_RADIUS: f32 = 3.5;
const LED_SPACING: f32 = 6.0;
const KNOB_DIAMETER: f32 = 26.0;
const UNREACHABLE_OPACITY: f32 = 0.55;
const METER_GROUP_GAP: f32 = 4.0;
const DELAY_PATH: &str = "audio/out/delay";
const STANDBY_TIME_PATH: &str = "device/standby/auto_standby_time";

pub struct StripResponse {
    /// The writes the user asked for that the write policy allowed.
    pub commands: Vec<Command>,
    /// Where the device panel opens when the name was clicked.
    pub open_panel: Option<Pos2>,
}

pub fn show(
    ui: &mut Ui,
    speaker: &mut SpeakerState,
    catalog: &Catalog,
    desk: &DeskSettings,
) -> StripResponse {
    let tokens = Tokens::current(ui.ctx());
    let mut commands = Vec::new();
    let inner_width = STRIP_WIDTH - 2.0 * (f32::from(STRIP_MARGIN) + BORDER_WIDTH);
    let open_panel = egui::Frame::new()
        .fill(tokens.card)
        .stroke(Stroke::new(BORDER_WIDTH, tokens.border))
        .corner_radius(CornerRadius::same(STRIP_RADIUS))
        .inner_margin(STRIP_MARGIN)
        .show(ui, |ui| {
            // The desk lays strips out in a row; the content of one strip stacks.
            ui.vertical(|ui| {
                ui.set_width(inner_width);
                if speaker.status == Status::Unreachable {
                    ui.multiply_opacity(UNREACHABLE_OPACITY);
                }
                let open_panel = name(ui, speaker);
                leds(ui, speaker, &tokens);
                knobs(ui, speaker, catalog, &mut commands);
                level(ui, speaker, catalog, desk, &mut commands, &tokens);
                if desk.show_response_times {
                    response_time(ui, speaker);
                }
                buttons(ui, speaker, catalog, &mut commands);
                message(ui, speaker, &tokens);
                open_panel
            })
            .inner
        })
        .inner;
    StripResponse {
        commands,
        open_panel,
    }
}

fn name(ui: &mut Ui, speaker: &SpeakerState) -> Option<Pos2> {
    let text = RichText::new(speaker.display_name()).strong();
    let response = ui.add(Button::new(text).frame(false).truncate());
    response.clicked().then(|| response.rect.left_bottom())
}

fn leds(ui: &mut Ui, speaker: &SpeakerState, tokens: &Tokens) {
    let state = status_leds(speaker);
    let connection = match state.connection {
        ConnectionLed::Connected => tokens.connected,
        ConnectionLed::Connecting => tokens.connecting,
        ConnectionLed::Off => tokens.unreachable,
    };
    let lit = |on: bool, color: Color32| if on { color } else { tokens.clip_off };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = LED_SPACING;
        led(ui, connection, texts::LED_CONNECTION);
        led(ui, lit(state.clip, tokens.danger), texts::LED_CLIP);
        led(
            ui,
            lit(state.warning, tokens.connecting),
            texts::LED_WARNING,
        );
        led(ui, lit(state.local, tokens.accent), texts::LED_LOCAL);
    });
}

fn led(ui: &mut Ui, color: Color32, text: &str) {
    let size = Vec2::splat(2.0 * LED_RADIUS);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().circle_filled(rect.center(), LED_RADIUS, color);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, text));
    response.on_hover_text(text);
}

fn knobs(ui: &mut Ui, speaker: &mut SpeakerState, catalog: &Catalog, commands: &mut Vec<Command>) {
    let product = speaker.product().to_owned();
    let entries = catalog.entries_for(&product);
    let entry = |path: &str| {
        let path = paths::path(path);
        entries.iter().copied().find(|entry| entry.path == path)
    };
    ui.columns(2, |columns| {
        let knobs = [
            (DELAY_PATH, texts::KNOB_DELAY),
            (STANDBY_TIME_PATH, texts::KNOB_STANDBY),
        ];
        for (column, (path, label)) in columns.iter_mut().zip(knobs) {
            let Some(entry) = entry(path) else {
                continue;
            };
            let sent = knob::for_entry(column, speaker, catalog, entry, label, true, KNOB_DIAMETER);
            commands.extend(sent);
        }
    });
}

fn level(
    ui: &mut Ui,
    speaker: &mut SpeakerState,
    catalog: &Catalog,
    desk: &DeskSettings,
    commands: &mut Vec<Command>,
    tokens: &Tokens,
) {
    let path = paths::path(paths::OUTPUT_LEVEL);
    let device = speaker.values.get(&path).and_then(Value::as_f64);
    let unit = paths::catalog_entry(catalog, &path)
        .and_then(|entry| entry.unit.clone())
        .unwrap_or_default();
    let shown = ui
        .horizontal(|ui| {
            let range = output_level_range();
            let shown = fader::show(ui, speaker, catalog, commands, device, range, &unit);
            meters(ui, speaker, desk, tokens);
            shown
        })
        .inner;
    let readout = device.map(|_| texts::readout(shown, &unit));
    ui.add(Label::new(
        RichText::new(readout.unwrap_or_default()).monospace(),
    ));
}

/// Input meters, then the output meters of models that report them (KH 750), set apart.
fn meters(ui: &mut Ui, speaker: &SpeakerState, desk: &DeskSettings, tokens: &Tokens) {
    let floor = desk.meter_floor.db();
    let input_clips = speaker.values.get(&paths::path(paths::INPUT_CLIP));
    let input = &speaker.input_meters;
    let group = texts::INPUT_METERS;
    meters::show(ui, group, input, input_clips, FADER_HEIGHT, floor, tokens);
    if speaker.output_meters.is_empty() {
        return;
    }
    ui.add_space(METER_GROUP_GAP);
    let output_clips = speaker.values.get(&paths::path(paths::OUTPUT_CLIP));
    let output = &speaker.output_meters;
    let group = texts::OUTPUT_METERS;
    meters::show(ui, group, output, output_clips, FADER_HEIGHT, floor, tokens);
}

fn response_time(ui: &mut Ui, speaker: &SpeakerState) {
    let text = speaker
        .response_time
        .map(|time| texts::response_time(time.as_millis()))
        .unwrap_or_default();
    ui.add(Label::new(RichText::new(text).small().weak().monospace()));
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

fn message(ui: &mut Ui, speaker: &SpeakerState, tokens: &Tokens) {
    if let Some(message) = &speaker.message {
        ui.add(Label::new(RichText::new(message).small().color(tokens.danger)).wrap());
    }
}
