//! The bands of one EQ container as a table: on/off, type, frequency, Q, gain and boost. Number
//! cells follow the device panel's gesture rules (`number_field`).

use std::ops::RangeInclusive;

use eframe::egui::{
    Checkbox, Color32, ComboBox, Grid, Id, RichText, Role, ScrollArea, Ui, WidgetInfo,
};
use serde_json::Value;

use super::number_field::{self, NumberField};
use crate::eq::{Leaf, GAIN_GRID_DB, Q_STEP};
use crate::texts;

const TABLE_HEIGHT: f32 = 260.0;
/// The order of the columns after the band number.
const COLUMNS: [Leaf; 6] = [
    Leaf::Enabled,
    Leaf::Type,
    Leaf::Frequency,
    Leaf::Q,
    Leaf::Gain,
    Leaf::Boost,
];
/// Frequencies are dragged in whole hertz; typing takes any value.
const FREQUENCY_DRAG_SPEED: f64 = 1.0;

pub struct TableRow {
    pub enabled: bool,
    pub kind: String,
    pub frequency: f64,
    pub q: f64,
    pub gain: f64,
    pub boost: f64,
}

/// What the catalog allows per leaf.
pub struct Limits {
    pub frequency: RangeInclusive<f64>,
    pub q: RangeInclusive<f64>,
    pub gain: RangeInclusive<f64>,
    pub types: Vec<String>,
    /// The catalog's units of frequency and of level and boost.
    pub frequency_unit: Option<String>,
    pub gain_unit: Option<String>,
}

pub struct Table<'a> {
    pub id: Id,
    pub rows: &'a [TableRow],
    pub limits: &'a Limits,
    /// Column titles in `Leaf::ALL` order, from the catalog in the current language.
    pub headers: &'a [String; 6],
    pub editable: bool,
    pub selected: usize,
    /// Each band's colour, as its point in the graph has it.
    pub colors: &'a [Color32],
}

#[derive(Clone, Debug, PartialEq)]
pub enum TableAction {
    Select(usize),
    Write {
        band: usize,
        leaf: Leaf,
        value: Value,
    },
}

pub fn show(ui: &mut Ui, table: &Table) -> Vec<TableAction> {
    let mut actions = Vec::new();
    ScrollArea::vertical()
        .id_salt(table.id)
        .max_height(TABLE_HEIGHT)
        .show(ui, |ui| {
            Grid::new(table.id).striped(true).show(ui, |ui| {
                ui.label(texts::BAND);
                let header_ids: Vec<(Leaf, Id)> = COLUMNS
                    .iter()
                    .map(|leaf| (*leaf, ui.label(header(table, *leaf)).id))
                    .collect();
                ui.end_row();
                for (band, row) in table.rows.iter().enumerate() {
                    actions.extend(row_cells(ui, table, &header_ids, band, row));
                    ui.end_row();
                }
            });
        });
    actions
}

fn header<'a>(table: &'a Table, leaf: Leaf) -> &'a str {
    let place = Leaf::ALL
        .iter()
        .position(|known| *known == leaf)
        .unwrap_or_default();
    &table.headers[place]
}

/// The column title a cell is named by for screen readers.
fn header_id(headers: &[(Leaf, Id)], leaf: Leaf) -> Id {
    headers
        .iter()
        .find(|(known, _)| *known == leaf)
        .map(|(_, id)| *id)
        .expect("every column has a title")
}

fn row_cells(
    ui: &mut Ui,
    table: &Table,
    headers: &[(Leaf, Id)],
    band: usize,
    row: &TableRow,
) -> Vec<TableAction> {
    let mut actions = Vec::new();
    let mut number = RichText::new((band + 1).to_string()).strong();
    if let Some(color) = table.colors.get(band) {
        number = number.color(*color);
    }
    if ui
        .selectable_label(band == table.selected, number)
        .clicked()
    {
        actions.push(TableAction::Select(band));
    }
    let mut enabled = row.enabled;
    let checkbox = ui
        .add_enabled(table.editable, Checkbox::without_text(&mut enabled))
        .labelled_by(header_id(headers, Leaf::Enabled));
    if checkbox.clicked() {
        actions.push(write(band, Leaf::Enabled, Value::Bool(enabled)));
    }
    actions.extend(type_cell(ui, table, band, row));
    let limits = table.limits;
    let cells = [
        (
            Leaf::Frequency,
            row.frequency,
            &limits.frequency,
            FREQUENCY_DRAG_SPEED,
            limits.frequency_unit.as_deref(),
        ),
        (Leaf::Q, row.q, &limits.q, Q_STEP, None),
        (
            Leaf::Gain,
            row.gain,
            &limits.gain,
            GAIN_GRID_DB,
            limits.gain_unit.as_deref(),
        ),
        (
            Leaf::Boost,
            row.boost,
            &limits.gain,
            GAIN_GRID_DB,
            limits.gain_unit.as_deref(),
        ),
    ];
    for (leaf, value, range, speed, unit) in cells {
        let field = NumberField {
            id: table.id.with((band, leaf.name())),
            device: Some(value),
            range: range.clone(),
            step: None,
            drag_speed: speed,
            unit,
            conversion: None,
            enabled: table.editable,
            labelled_by: Some(header_id(headers, leaf)),
        };
        if let Some(number) = number_field::show(ui, &field) {
            actions.push(write(band, leaf, Value::from(number)));
        }
    }
    actions
}

fn type_cell(ui: &mut Ui, table: &Table, band: usize, row: &TableRow) -> Option<TableAction> {
    let known = table.limits.types.contains(&row.kind);
    let mut chosen = None;
    // A type the catalog does not know is shown as it is and left alone.
    // Named by its band, as the column title says it: the table shows no label beside it.
    let name = texts::eq_band_leaf(band + 1, header(table, Leaf::Type));
    ui.add_enabled_ui(table.editable && known, |ui| {
        let choice = ComboBox::from_id_salt(table.id.with((band, Leaf::Type.name())))
            .selected_text(&row.kind)
            .show_ui(ui, |ui| {
                for option in &table.limits.types {
                    let selected = *option == row.kind;
                    if ui.selectable_label(selected, option).clicked() && !selected {
                        chosen = Some(option.clone());
                    }
                }
            });
        let enabled = table.editable && known;
        choice
            .response
            .widget_info(|| WidgetInfo::labeled(Role::ComboBox, enabled, &name));
    });
    chosen.map(|kind| write(band, Leaf::Type, Value::String(kind)))
}

fn write(band: usize, leaf: Leaf, value: Value) -> TableAction {
    TableAction::Write { band, leaf, value }
}
