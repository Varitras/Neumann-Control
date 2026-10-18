//! One speaker as a channel strip: name (opens the device panel), status LEDs, delay and
//! standby knobs, level fader with meters, readout, response time, mute and identify, and the
//! last message. The strip only reports gestures; `writes` decides what becomes a command.

use eframe::egui::{
    self, vec2, Align, Button, Color32, CornerRadius, Label, Layout, Pos2, Rect, RichText, Sense,
    Stroke, Ui, Vec2, WidgetInfo, WidgetType,
};
use neumann_control_core::catalog::Catalog;
use neumann_control_core::settings::DeskSettings;
use neumann_control_core::speaker::Command;
use serde_json::Value;

use super::fader::{self, FADER_HEIGHT};
use super::knob;
use super::meters::{self, Meters};
use crate::fader::{output_level_range, WHEEL_STEP_DB};
use crate::input_source::input_source;
use crate::paths;
use crate::state::{SpeakerState, Status};
use crate::status_leds::{status_leds, ConnectionLed};
use crate::texts;
use crate::ui_kit::theme::Tokens;
use crate::writes::{self, Intent, Switch};

pub const STRIP_WIDTH: f32 = 140.0;
const STRIP_MARGIN: i8 = 6;
const STRIP_RADIUS: u8 = 6;
const GROUP_BORDER_WIDTH: f32 = 2.0;
const GRIP_ID: &str = "grip";
const GRIP_ROWS: usize = 3;
const GRIP_DOT_SPACING: f32 = 4.0;
const GRIP_DOT_RADIUS: f32 = 1.2;
const GRIP_WIDTH: f32 = 2.0 * GRIP_DOT_SPACING;
const GRIP_HEIGHT: f32 = GRIP_ROWS as f32 * GRIP_DOT_SPACING;
const BORDER_WIDTH: f32 = 1.0;
const LED_RADIUS: f32 = 3.5;
const LED_SPACING: f32 = 6.0;
const UNREACHABLE_OPACITY: f32 = 0.55;
const METER_GROUP_GAP: f32 = 4.0;
const METER_SPACING: f32 = 3.0;
const CAPTION_HEIGHT: f32 = 16.0;
const DELAY_PATH: &str = "audio/out/delay";
const LEVEL_ENTRY_ID: &str = "level_entry";
const STANDBY_TIME_PATH: &str = "device/standby/auto_standby_time";

pub struct StripResponse {
    /// The writes the user asked for that the write policy allowed.
    pub commands: Vec<Command>,
    /// Where the device panel opens when the name was clicked.
    pub open_panel: Option<Pos2>,
    /// The strip's area, where another strip can be dropped.
    pub rect: Rect,
}

pub fn show(
    ui: &mut Ui,
    speaker: &mut SpeakerState,
    catalog: &Catalog,
    desk: &DeskSettings,
    group_color: Option<Color32>,
    member: &str,
) -> StripResponse {
    let tokens = Tokens::current(ui.ctx());
    let mut commands = Vec::new();
    let inner_width = STRIP_WIDTH - 2.0 * (f32::from(STRIP_MARGIN) + BORDER_WIDTH);
    let frame = egui::Frame::new()
        .fill(tokens.card)
        // A member of a group is framed in the group's colour.
        .stroke(
            group_color.map_or(Stroke::new(BORDER_WIDTH, tokens.border), |color| {
                Stroke::new(GROUP_BORDER_WIDTH, color)
            }),
        )
        .corner_radius(CornerRadius::same(STRIP_RADIUS))
        .inner_margin(STRIP_MARGIN)
        .show(ui, |ui| {
            // The desk lays strips out in a row; the content of one strip stacks.
            ui.vertical(|ui| {
                ui.set_width(inner_width);
                if speaker.status == Status::Unreachable {
                    ui.multiply_opacity(UNREACHABLE_OPACITY);
                }
                let open_panel = ui
                    .horizontal(|ui| {
                        grip(ui, member, &tokens);
                        name(ui, speaker)
                    })
                    .inner;
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
        });
    StripResponse {
        commands,
        open_panel: frame.inner,
        rect: frame.response.rect,
    }
}

/// The handle a strip is dragged by to another place on the desk; the payload is the speaker's
/// identity in the saved order.
fn grip(ui: &mut Ui, member: &str, tokens: &Tokens) {
    let id = ui.id().with(GRIP_ID);
    let response = ui
        .dnd_drag_source(id, member.to_owned(), |ui| {
            let size = vec2(GRIP_WIDTH, GRIP_HEIGHT);
            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
            for row in 0..GRIP_ROWS {
                for column in 0..2 {
                    let center = rect.left_top()
                        + vec2(
                            GRIP_DOT_SPACING * (column as f32 + 0.5),
                            GRIP_DOT_SPACING * (row as f32 + 0.5),
                        );
                    ui.painter()
                        .circle_filled(center, GRIP_DOT_RADIUS, tokens.text_dim);
                }
            }
        })
        .response;
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, texts::MOVE_STRIP));
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
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
            let sent = knob::for_entry(
                column,
                speaker,
                catalog,
                entry,
                label,
                true,
                knob::KNOB_DIAMETER,
            );
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
    // Top-aligned: the meter groups are taller than the fader (names, caption) and a centred
    // row would shift them against it and against each other.
    let shown = ui
        .with_layout(Layout::left_to_right(Align::Min), |ui| {
            let range = output_level_range();
            let shown = fader::show(ui, speaker, catalog, commands, device, range, &unit);
            meters(ui, speaker, desk, tokens);
            shown
        })
        .inner;
    let readout = device
        .map(|_| texts::readout(shown, &unit))
        .unwrap_or_default();
    let label = Label::new(RichText::new(readout).monospace()).sense(Sense::click());
    let response = ui.add(label).on_hover_text(texts::CONTROL_HINT);
    let entry_id = response.id.with(LEVEL_ENTRY_ID);
    let open_now = response.double_clicked() && device.is_some();
    let range = output_level_range();
    if let Some(level) =
        knob::value_entry(ui, entry_id, open_now, shown, &range, WHEEL_STEP_DB, None)
    {
        let now = ui.input(|input| input.time);
        commands.extend(speaker.request(Intent::Level(level), catalog, now));
    }
}

/// Input meters, then the output meters of models that report them (KH 750), set apart.
fn meters(ui: &mut Ui, speaker: &SpeakerState, desk: &DeskSettings, tokens: &Tokens) {
    let source = input_source(speaker);
    let caption = source.map_or(texts::METER_INPUT_SHORT, |source| source.caption);
    let played = source.map(|source| source.played).unwrap_or_default();
    let played: &[bool] = if source.is_some() { &played } else { &[] };
    let floor = desk.meter_floor.db();
    let input = MeterGroup {
        with_scale: true,
        caption,
        meters: Meters {
            name: texts::INPUT_METERS,
            channels: texts::INPUT_CHANNELS,
            levels: &speaker.input_meters,
            clips: speaker.values.get(&paths::path(paths::INPUT_CLIP)),
            played,
        },
    };
    meter_group(ui, &input, floor, tokens);
    if speaker.output_meters.is_empty() {
        return;
    }
    ui.add_space(METER_GROUP_GAP);
    let output = MeterGroup {
        with_scale: false,
        caption: texts::METER_OUTPUT_SHORT,
        meters: Meters {
            name: texts::OUTPUT_METERS,
            channels: texts::OUTPUT_CHANNELS,
            levels: &speaker.output_meters,
            clips: speaker.values.get(&paths::path(paths::OUTPUT_CLIP)),
            played: &[],
        },
    };
    meter_group(ui, &output, floor, tokens);
}

struct MeterGroup<'a> {
    /// The first group carries the dB marks; the others share its heights.
    with_scale: bool,
    /// Under the meters, so input and output can be told apart at a glance.
    caption: &'a str,
    meters: Meters<'a>,
}

fn meter_group(ui: &mut Ui, group: &MeterGroup, floor: f64, tokens: &Tokens) {
    ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
        ui.spacing_mut().item_spacing.x = METER_SPACING;
        if group.with_scale {
            meters::scale(ui, FADER_HEIGHT, floor, tokens);
        }
        // The caption belongs to the meters, not to the scale beside them.
        ui.vertical(|ui| {
            let meters = ui
                .horizontal(|ui| meters::show(ui, &group.meters, FADER_HEIGHT, floor, tokens))
                .inner;
            // Centred on the meters but never wrapped: "Analog" is wider than two meters.
            let caption = RichText::new(group.caption).small().weak();
            let width = ui
                .painter()
                .layout_no_wrap(caption.text().to_owned(), small_font(ui), tokens.text_dim)
                .size()
                .x;
            let size = vec2(meters.width(), CAPTION_HEIGHT);
            let (reserved, _) = ui.allocate_exact_size(size, Sense::hover());
            let row = Rect::from_center_size(
                egui::pos2(meters.center().x, reserved.center().y),
                vec2(width.max(meters.width()), CAPTION_HEIGHT),
            );
            ui.put(row, Label::new(caption).extend());
        });
    });
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

fn small_font(ui: &Ui) -> egui::FontId {
    egui::TextStyle::Small.resolve(ui.style())
}
