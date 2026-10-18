//! The device panel's settings: every catalog entry for the speaker's model, in the catalog's
//! group order. Entries the model may write get a control (numbers also a knob), the rest a
//! greyed value; what the strip or the panel header shows and what the speaker does not have
//! are left out.

use eframe::egui::{Button, RichText, Ui};
use neumann_control_core::catalog::{Catalog, Entry, Group, Kind};
use neumann_control_core::speaker::Command;

use super::details_fields::Fields;
use super::knob;
use crate::display::shown_value;
use crate::paths;
use crate::state::{AllValuesState, SpeakerState, Status};
use crate::texts;

const COLUMNS: usize = 2;
const GROUP_GAP: f32 = 8.0;

/// Drawn by the strip (level, mute, identify, meters) or the panel header (model, firmware,
/// serial).
const SHOWN_ELSEWHERE: &[&str] = &[
    paths::OUTPUT_LEVEL,
    paths::MUTE,
    paths::IDENTIFY,
    paths::INPUT_METER,
    paths::INPUT_CLIP,
    paths::PRODUCT,
    paths::FIRMWARE_VERSION,
    paths::SERIAL,
];

/// Draws the area and returns the writes the user asked for that the write policy allowed.
pub fn show(ui: &mut Ui, speaker: &mut SpeakerState, catalog: &Catalog) -> Vec<Command> {
    ui.vertical(|ui| rows(ui, speaker, catalog)).inner
}

fn rows(ui: &mut Ui, speaker: &mut SpeakerState, catalog: &Catalog) -> Vec<Command> {
    let product = speaker.product().to_owned();
    let groups = groups(catalog.entries_for(&product), speaker);
    let mut commands = Vec::new();
    let mut fields = Fields {
        speaker,
        catalog,
        commands: &mut commands,
    };
    let (left, right) = split_in_columns(groups);
    ui.columns(COLUMNS, |columns| {
        for (column, groups) in columns.iter_mut().zip([left, right]) {
            for (group, entries) in groups {
                column.push_id(group.title(texts::language()), |ui| {
                    ui.label(RichText::new(group.title(texts::language())).strong());
                    for entry in entries {
                        row(ui, &mut fields, entry);
                    }
                    ui.add_space(GROUP_GAP);
                });
            }
        }
    });
    ui.separator();
    all_values_button(ui, fields.speaker, fields.commands);
    commands
}

type Groups<'a> = Vec<(Group, Vec<&'a Entry>)>;

/// Groups in catalog order, the first ones left until about half the rows are placed.
fn split_in_columns(groups: Groups<'_>) -> (Groups<'_>, Groups<'_>) {
    let total: usize = groups.iter().map(|(_, entries)| entries.len()).sum();
    let mut placed = 0;
    let mut left = Vec::new();
    let mut right = Vec::new();
    for (group, entries) in groups {
        let fits_left = right.is_empty() && 2 * placed < total;
        placed += entries.len();
        if fits_left {
            left.push((group, entries));
        } else {
            right.push((group, entries));
        }
    }
    (left, right)
}

/// The entries to show, grouped in the order the catalog first names each group.
fn groups<'a>(entries: Vec<&'a Entry>, speaker: &SpeakerState) -> Groups<'a> {
    let mut groups: Groups = Vec::new();
    for entry in entries.into_iter().filter(|entry| is_shown(entry, speaker)) {
        match groups.iter_mut().find(|(group, _)| *group == entry.group) {
            Some((_, members)) => members.push(entry),
            None => groups.push((entry.group, vec![entry])),
        }
    }
    groups
}

/// Meters sit on the strip and clip lists light their lamps, so neither gets a row here.
fn is_shown(entry: &Entry, speaker: &SpeakerState) -> bool {
    let path = entry.path.to_string();
    let on_strip = SHOWN_ELSEWHERE.contains(&path.as_str());
    let absent = speaker.absent.contains(&entry.path);
    let drawable = !matches!(entry.kind, Kind::Clip | Kind::Meter);
    drawable && !on_strip && !absent
}

fn row(ui: &mut Ui, fields: &mut Fields, entry: &Entry) {
    let product = fields.speaker.product();
    if !entry.is_writable(product) {
        display(ui, fields.speaker, entry);
        return;
    }
    match entry.kind {
        Kind::Number => {
            ui.horizontal(|ui| {
                let catalog = fields.catalog;
                let label = texts::knob_label(entry.label(texts::language()));
                let speaker = &mut *fields.speaker;
                let sent = knob::for_entry(
                    ui,
                    speaker,
                    catalog,
                    entry,
                    &label,
                    false,
                    knob::KNOB_DIAMETER,
                );
                fields.commands.extend(sent);
                fields.number(ui, entry);
            });
        }
        Kind::Switch => fields.switch(ui, entry),
        Kind::Select => fields.select(ui, entry),
        Kind::Text => fields.text(ui, entry),
        Kind::Display | Kind::Meter | Kind::Clip => display(ui, fields.speaker, entry),
    }
}

fn display(ui: &mut Ui, speaker: &SpeakerState, entry: &Entry) {
    let shown = speaker
        .values
        .get(&entry.path)
        .map(|value| shown_value(entry, value))
        .unwrap_or_default();
    ui.horizontal_wrapped(|ui| {
        ui.weak(entry.label(texts::language()));
        ui.weak(shown);
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
