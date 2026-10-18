//! The card's "Mehr" area: every catalog entry for the speaker's model, in the catalog's group
//! order. Entries the model may write get a control, the rest a greyed value; what the card
//! itself shows and what the speaker does not have are left out.

use eframe::egui::{Button, RichText, Ui};
use neumann_control_core::catalog::{Catalog, Entry, Group, Kind};
use neumann_control_core::speaker::Command;

use super::details_fields::Fields;
use super::meters;
use crate::display::shown_value;
use crate::paths;
use crate::state::{AllValuesState, SpeakerState, Status};
use crate::texts;
use crate::ui_kit::theme::Tokens;

/// Drawn by the card itself: level, mute, identify, input meters, model, firmware and serial.
const SHOWN_ON_CARD: &[&str] = &[
    paths::OUTPUT_LEVEL,
    paths::MUTE,
    paths::IDENTIFY,
    paths::INPUT_METER,
    paths::INPUT_CLIP,
    paths::PRODUCT,
    paths::FIRMWARE_VERSION,
    paths::SERIAL,
];
const OUTPUT_METER_HEIGHT: f32 = 60.0;

/// Draws the area and returns the writes the user asked for that the write policy allowed.
pub fn show(ui: &mut Ui, speaker: &mut SpeakerState, catalog: &Catalog) -> Vec<Command> {
    // The card's own layout wraps sideways; the rows of this area stack.
    ui.vertical(|ui| rows(ui, speaker, catalog)).inner
}

fn rows(ui: &mut Ui, speaker: &mut SpeakerState, catalog: &Catalog) -> Vec<Command> {
    let tokens = Tokens::current(ui.ctx());
    let product = speaker.product().to_owned();
    let groups = groups(catalog.entries_for(&product), speaker);
    let mut commands = Vec::new();
    let mut fields = Fields {
        speaker,
        catalog,
        commands: &mut commands,
    };
    for (group, entries) in groups {
        ui.push_id(group.title(), |ui| {
            ui.separator();
            ui.label(RichText::new(group.title()).strong());
            for entry in entries {
                row(ui, &mut fields, entry, &tokens);
            }
        });
    }
    ui.separator();
    all_values_button(ui, fields.speaker, fields.commands);
    commands
}

/// The entries to show, grouped in the order the catalog first names each group.
fn groups<'a>(entries: Vec<&'a Entry>, speaker: &SpeakerState) -> Vec<(Group, Vec<&'a Entry>)> {
    let mut groups: Vec<(Group, Vec<&Entry>)> = Vec::new();
    for entry in entries.into_iter().filter(|entry| is_shown(entry, speaker)) {
        match groups.iter_mut().find(|(group, _)| *group == entry.group) {
            Some((_, members)) => members.push(entry),
            None => groups.push((entry.group, vec![entry])),
        }
    }
    groups
}

/// Clip lists light the lamps of their meters, so they get no row of their own.
fn is_shown(entry: &Entry, speaker: &SpeakerState) -> bool {
    let path = entry.path.to_string();
    let on_card = SHOWN_ON_CARD.contains(&path.as_str());
    let absent = speaker.absent.contains(&entry.path);
    let drawable = match entry.kind {
        Kind::Clip => false,
        Kind::Meter => path == paths::OUTPUT_METER,
        _ => true,
    };
    drawable && !on_card && !absent
}

fn row(ui: &mut Ui, fields: &mut Fields, entry: &Entry, tokens: &Tokens) {
    let product = fields.speaker.product();
    if !entry.is_writable(product) {
        display(ui, fields.speaker, entry, tokens);
        return;
    }
    match entry.kind {
        Kind::Number => fields.number(ui, entry),
        Kind::Switch => fields.switch(ui, entry),
        Kind::Select => fields.select(ui, entry),
        Kind::Text => fields.text(ui, entry),
        Kind::Display | Kind::Meter | Kind::Clip => display(ui, fields.speaker, entry, tokens),
    }
}

fn display(ui: &mut Ui, speaker: &SpeakerState, entry: &Entry, tokens: &Tokens) {
    if entry.kind == Kind::Meter {
        output_meters(ui, speaker, entry, tokens);
        return;
    }
    let shown = speaker
        .values
        .get(&entry.path)
        .map(|value| shown_value(entry, value))
        .unwrap_or_default();
    ui.horizontal_wrapped(|ui| {
        ui.weak(&entry.label);
        ui.weak(shown);
    });
}

fn output_meters(ui: &mut Ui, speaker: &SpeakerState, entry: &Entry, tokens: &Tokens) {
    ui.weak(&entry.label);
    let clips = speaker.values.get(&paths::path(paths::OUTPUT_CLIP));
    ui.horizontal(|ui| {
        meters::show(
            ui,
            &speaker.output_meters,
            clips,
            OUTPUT_METER_HEIGHT,
            tokens,
        );
    });
}

fn all_values_button(ui: &mut Ui, speaker: &mut SpeakerState, commands: &mut Vec<Command>) {
    let loading = speaker.all_values == AllValuesState::Loading;
    let enabled = speaker.status == Status::Connected && !loading;
    if ui
        .add_enabled(enabled, Button::new(texts::ALL_VALUES))
        .clicked()
    {
        commands.push(speaker.read_all_values());
    }
}
