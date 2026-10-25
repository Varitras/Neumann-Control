//! The compact mode's rows: LEDs (or the group's colour), name, a lying level fader with its
//! value, the loudest input as a small meter, then buttons: on a speaker's row the ones chosen
//! for its model, on a group's and the master's mute. One row per ungrouped speaker and per
//! group, the master at the bottom. The rows only report gestures, as the strips do.

use std::ops::RangeInclusive;

use eframe::egui::{
    self, vec2, Align, Button, Color32, CornerRadius, Layout, RichText, Role, Sense, Stroke, Ui,
    WidgetInfo,
};
use neumann_control_core::catalog::Catalog;
use neumann_control_core::settings::{CompactButton, DeskSettings, GroupSettings};
use neumann_control_core::speaker::Command;
use neumann_control_core::ssc::wire::SscPath;
use serde_json::Value;

use super::compact_menu;
use super::fader::{self, Orientation, Scale};
use super::group_strip::{mute_button, shared_readout, GroupStripResponse, MasterResponse};
use super::layout_menu::{popup, right_clicked};
use super::meters;
use super::named_region;
use super::strip::{self, UNREACHABLE_OPACITY};
use crate::compact_layout::CompactChange;
use crate::fader::output_level_range;
use crate::groups::SharedLevel;
use crate::paths;
use crate::state::{SpeakerState, Status};
use crate::texts;
use crate::ui_kit::theme::Tokens;

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
const GROUP_MENU_ID: &str = "compact_group_menu";

pub struct SpeakerRowResponse {
    /// The writes the user asked for that the write policy allowed.
    pub commands: Vec<Command>,
    /// The name or Details was clicked: the speaker's panel opens.
    pub open_panel: bool,
    pub open_eq: bool,
    /// A change of the model's compact buttons.
    pub layout: Option<CompactChange>,
}

/// `color`: the speaker's own strip colour for the row's frame; without one the plain border.
pub fn speaker(
    ui: &mut Ui,
    speaker: &mut SpeakerState,
    catalog: &Catalog,
    desk: &DeskSettings,
    buttons: &[CompactButton],
    color: Option<Color32>,
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
    // Before the model is known there is nothing to offer that it surely has.
    let offers = speaker
        .identity
        .is_some()
        .then(|| compact_menu::offers(buttons, speaker.product(), catalog));
    let mut open_eq = false;
    let mut layout = None;
    let name = speaker.display_name();
    let open_panel = row(ui, &name, color.unwrap_or(tokens.border), |ui| {
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
        let mut open_details = false;
        let mut rects = Vec::new();
        for button in buttons {
            let response = match button {
                CompactButton::Switch(path) => {
                    let text = switch_text(path, catalog);
                    let path = path.clone();
                    strip::toggle(ui, speaker, catalog, &mut commands, path, &text, true)
                }
                CompactButton::Eq => {
                    let eq = ui.button(texts::EQ);
                    open_eq |= eq.clicked();
                    eq
                }
                CompactButton::Details => {
                    let details = ui.button(texts::DETAILS);
                    open_details |= details.clicked();
                    details
                }
            };
            if let Some(offers) = &offers {
                let change = compact_menu::button_menu(ui, response.rect, button, offers);
                layout = change.or(layout.take());
            }
            rects.push(response.rect);
        }
        if let Some(message) = &speaker.message {
            message_mark(ui, &texts::describe_message(message), &tokens);
        }
        if let Some(offers) = &offers {
            let change = compact_menu::row_menu(ui, ui.min_rect(), &rects, offers);
            layout = change.or(layout.take());
        }
        open_panel || open_details
    });
    SpeakerRowResponse {
        commands,
        open_panel,
        open_eq,
        layout,
    }
}

/// Short as on the strip, so the row stays one line.
fn switch_text(path: &SscPath, catalog: &Catalog) -> String {
    paths::catalog_entry(catalog, path).map_or_else(
        || path.to_string(),
        |entry| strip::button_text(entry).to_owned(),
    )
}

/// What a group or master row shows: the level of the loudest connected member, whether all are
/// muted, and the loudest input among them.
pub struct SharedRow {
    pub level: Option<SharedLevel>,
    pub muted: Option<bool>,
    pub loudest: Option<f64>,
    pub range: RangeInclusive<f64>,
    pub floor: f64,
    /// A member's message, worded with its name: grouped members have no row of their own.
    pub message: Option<String>,
}

pub fn group(
    ui: &mut Ui,
    group: &GroupSettings,
    color: Option<Color32>,
    shown: SharedRow,
) -> GroupStripResponse {
    let tokens = Tokens::current(ui.ctx());
    let mut response = GroupStripResponse::default();
    row(ui, &group.name, color.unwrap_or(tokens.border), |ui| {
        cell(ui, MARK_WIDTH, |ui| {
            if let Some(color) = color {
                swatch(ui, color);
            }
        });
        let mut name = RichText::new(&group.name).strong();
        if let Some(color) = color {
            name = name.color(color);
        }
        response.open_settings = cell(ui, NAME_WIDTH, |ui| {
            ui.add(Button::new(name).frame(false).truncate()).clicked()
        });
        let label = texts::group_level(&group.name);
        let (level, mute) =
            shared_controls(ui, &label, &texts::group_mute(&group.name), shown, &tokens);
        response.level = level;
        response.mute = mute;
        // As on the group strip: the dialog asks before the group goes.
        let menu = ui.scope_id().with(GROUP_MENU_ID);
        let row_rect = ui.min_rect();
        popup(ui, menu, right_clicked(ui, row_rect), |ui| {
            response.delete = ui.button(texts::DELETE_GROUP_MENU).clicked();
        });
    });
    response
}

pub fn master(ui: &mut Ui, shown: SharedRow) -> MasterResponse {
    let tokens = Tokens::current(ui.ctx());
    let mut response = MasterResponse::default();
    row(ui, texts::MASTER.get(), tokens.accent, |ui| {
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
    if let Some(message) = &shown.message {
        message_mark(ui, message, tokens);
    }
    (moved, mute)
}

/// One row, named after what it shows (`name`) for screen readers.
fn row<R>(ui: &mut Ui, name: &str, border: Color32, contents: impl FnOnce(&mut Ui) -> R) -> R {
    named_region(ui, &texts::row_region(name), |ui| {
        framed_row(ui, border, contents)
    })
}

fn framed_row<R>(ui: &mut Ui, border: Color32, contents: impl FnOnce(&mut Ui) -> R) -> R {
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
    response.widget_info(|| WidgetInfo::labeled(Role::Label, true, text));
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
