//! The compact mode's rows: LEDs (or the group's colour), name, a lying level fader with its
//! value, the loudest input as a small meter, and mute. One row per ungrouped speaker and per
//! group, the master at the bottom. The rows only report gestures, as the strips do.

use std::ops::RangeInclusive;

use eframe::egui::{
    self, vec2, Align, Button, Color32, CornerRadius, Layout, Pos2, RichText, Sense, Stroke, Ui,
    WidgetInfo, WidgetType,
};
use neumann_control_core::catalog::Catalog;
use neumann_control_core::settings::{DeskSettings, GroupSettings};
use neumann_control_core::speaker::Command;
use serde_json::Value;

use super::fader::{self, Orientation, Scale};
use super::group_strip::{mute_button, shared_readout, GroupStripResponse, MasterResponse};
use super::meters;
use super::strip::{self, UNREACHABLE_OPACITY};
use crate::fader::output_level_range;
use crate::groups::SharedLevel;
use crate::paths;
use crate::state::{SpeakerState, Status};
use crate::texts;
use crate::ui_kit::theme::Tokens;
use crate::writes::Switch;

const ROW_MARGIN: i8 = 4;
const ROW_RADIUS: u8 = 4;
const BORDER_WIDTH: f32 = 1.0;
/// The columns line up from row to row; the LED column is as wide as four LEDs.
const MARK_WIDTH: f32 = 50.0;
const NAME_WIDTH: f32 = 120.0;
/// Wide enough for the longest readout, "120.0 dB SPL".
const READOUT_WIDTH: f32 = 96.0;
const ROW_HEIGHT: f32 = 22.0;
const SWATCH_SIZE: f32 = 10.0;
const SWATCH_RADIUS: u8 = 2;
const MESSAGE_MARK: &str = "!";

pub struct SpeakerRowResponse {
    /// The writes the user asked for that the write policy allowed.
    pub commands: Vec<Command>,
    /// Where the device panel opens when the name was clicked.
    pub open_panel: Option<Pos2>,
}

pub fn speaker(
    ui: &mut Ui,
    speaker: &mut SpeakerState,
    catalog: &Catalog,
    desk: &DeskSettings,
) -> SpeakerRowResponse {
    let tokens = Tokens::current(ui.ctx());
    let mut commands = Vec::new();
    let path = paths::path(paths::OUTPUT_LEVEL);
    let device = speaker.values.get(&path).and_then(Value::as_f64);
    let unit = paths::catalog_entry(catalog, &path)
        .and_then(|entry| entry.unit.clone())
        .unwrap_or_default();
    let now = ui.input(|input| input.time);
    let loudest = speaker.loudest_input(now);
    let open_panel = row(ui, tokens.border, |ui| {
        if speaker.status == Status::Unreachable {
            ui.multiply_opacity(UNREACHABLE_OPACITY);
        }
        cell(ui, MARK_WIDTH, |ui| strip::leds(ui, speaker, &tokens));
        let open_panel = cell(ui, NAME_WIDTH, |ui| strip::name(ui, speaker));
        let range = output_level_range();
        let scale = Scale {
            range,
            unit: &unit,
            orientation: Orientation::Horizontal,
        };
        let shown = fader::show(ui, speaker, catalog, &mut commands, device, scale);
        let readout = device
            .map(|_| texts::readout(shown, &unit))
            .unwrap_or_default();
        cell(ui, READOUT_WIDTH, |ui| readout_label(ui, readout));
        let floor = desk.meter_floor.db();
        meters::bar(ui, loudest, floor, texts::INPUT_METERS.get(), &tokens);
        let mute = texts::MUTE.get();
        strip::toggle(ui, speaker, catalog, &mut commands, Switch::Mute, mute);
        if let Some(message) = &speaker.message {
            message_mark(ui, &texts::describe_message(message), &tokens);
        }
        open_panel
    });
    SpeakerRowResponse {
        commands,
        open_panel,
    }
}

/// What a group or master row shows: the level of the loudest connected member, whether all are
/// muted, and the loudest input among them.
pub struct SharedRow {
    pub level: Option<SharedLevel>,
    pub muted: Option<bool>,
    pub loudest: Option<f64>,
    pub range: RangeInclusive<f64>,
    pub floor: f64,
}

pub fn group(
    ui: &mut Ui,
    group: &GroupSettings,
    color: Color32,
    shown: SharedRow,
) -> GroupStripResponse {
    let tokens = Tokens::current(ui.ctx());
    let mut response = GroupStripResponse::default();
    row(ui, color, |ui| {
        cell(ui, MARK_WIDTH, |ui| swatch(ui, color));
        let name = RichText::new(&group.name).strong().color(color);
        response.open_settings = cell(ui, NAME_WIDTH, |ui| {
            ui.add(Button::new(name).frame(false).truncate()).clicked()
        });
        let label = texts::group_level(&group.name);
        let (level, mute) =
            shared_controls(ui, &label, &texts::group_mute(&group.name), shown, &tokens);
        response.level = level;
        response.mute = mute;
    });
    response
}

pub fn master(ui: &mut Ui, shown: SharedRow) -> MasterResponse {
    let tokens = Tokens::current(ui.ctx());
    let mut response = MasterResponse::default();
    row(ui, tokens.accent, |ui| {
        cell(ui, MARK_WIDTH, |_| {});
        cell(ui, NAME_WIDTH, |ui| {
            ui.add(egui::Label::new(RichText::new(texts::MASTER).strong()))
        });
        let (level, mute) = shared_controls(
            ui,
            texts::MASTER_LEVEL.get(),
            texts::MUTE_ALL.get(),
            shown,
            &tokens,
        );
        response.level = level;
        response.mute = mute;
    });
    response
}

/// Fader, value, meter and mute of a row that moves several speakers at once.
fn shared_controls(
    ui: &mut Ui,
    level_label: &str,
    mute_label: &str,
    shown: SharedRow,
    tokens: &Tokens,
) -> (Option<f64>, Option<bool>) {
    let level = shown.level.as_ref();
    let range = shown.range;
    let moved = fader::shared(ui, level_label, level, range, Orientation::Horizontal);
    let readout = shared_readout(level);
    cell(ui, READOUT_WIDTH, |ui| readout_label(ui, readout));
    meters::bar(
        ui,
        shown.loudest,
        shown.floor,
        texts::INPUT_METERS.get(),
        tokens,
    );
    let mute = mute_button(ui, mute_label, shown.muted);
    (moved, mute)
}

fn row<R>(ui: &mut Ui, border: Color32, contents: impl FnOnce(&mut Ui) -> R) -> R {
    let tokens = Tokens::current(ui.ctx());
    egui::Frame::new()
        .fill(tokens.card)
        .stroke(Stroke::new(BORDER_WIDTH, border))
        .corner_radius(CornerRadius::same(ROW_RADIUS))
        .inner_margin(ROW_MARGIN)
        // `horizontal` starts one line high; a centred layout would fill all the height left.
        .show(ui, |ui| ui.horizontal(contents).inner)
        .inner
}

/// A column of fixed width, so that the rows line up.
fn cell<R>(ui: &mut Ui, width: f32, contents: impl FnOnce(&mut Ui) -> R) -> R {
    let size = vec2(width, ROW_HEIGHT);
    ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_size(size);
        contents(ui)
    })
    .inner
}

/// The strip's message line has no room in a row; a mark stands for it, the text on hover.
fn message_mark(ui: &mut Ui, text: &str, tokens: &Tokens) {
    let mark = RichText::new(MESSAGE_MARK).strong().color(tokens.danger);
    let response = ui.add(egui::Label::new(mark).sense(Sense::hover()));
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, text));
    response.on_hover_text(text);
}

/// Cut rather than pushing the columns behind it out of line.
fn readout_label(ui: &mut Ui, readout: String) {
    ui.add(egui::Label::new(RichText::new(readout).monospace()).truncate());
}

fn swatch(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(SWATCH_SIZE, SWATCH_SIZE), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(SWATCH_RADIUS), color);
}
