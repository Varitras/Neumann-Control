//! The bands of one EQ container as a table: on/off, type, frequency, Q, gain and boost. Bands
//! that do not fit below each other go into further columns to the right while the window is
//! wide enough, else the table scrolls. Number cells follow the device panel's gesture rules
//! (`number_field`).

use std::ops::RangeInclusive;

use eframe::egui::{
    Checkbox, Color32, ComboBox, Grid, Id, RichText, Role, ScrollArea, Ui, WidgetInfo,
};
use serde_json::Value;

use super::number_field::{self, NumberField};
use crate::eq::{Leaf, GAIN_GRID_DB, Q_STEP};
use crate::texts;

const TABLE_HEIGHT: f32 = 260.0;
/// Where a table keeps its measured column (width, row height) for the next frame.
const COLUMN_SIZE_ID: &str = "eq_table_column";
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
    let size_id = table.id.with(COLUMN_SIZE_ID);
    // A column's size is known once it was drawn: the first frame shows one column.
    let measured: Option<(f32, f32)> = ui.data(|data| data.get_temp(size_id));
    let gap = ui.spacing().item_spacing.x;
    // What is seen, not what the layout would grow to: a window that grows with its contents
    // would otherwise take columns it cannot show.
    let visible = ui
        .available_width()
        .min(ui.clip_rect().right() - ui.cursor().left());
    let columns = measured.map_or(1, |(width, row_height)| {
        let fitting = ((visible + gap) / (width + gap)).floor() as usize;
        let height = ui.available_height().min(TABLE_HEIGHT);
        column_count(
            table.rows.len(),
            rows_per_column(row_height, height),
            fitting,
        )
    });
    let per_column = table.rows.len().div_ceil(columns).max(1);
    let mut first_column = None;
    ScrollArea::vertical()
        .id_salt(table.id)
        .max_height(TABLE_HEIGHT)
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                for (column, start) in (0..table.rows.len()).step_by(per_column).enumerate() {
                    let bands = start..(start + per_column).min(table.rows.len());
                    let shown = Grid::new(table.id.with(column))
                        .striped(true)
                        .show(ui, |ui| {
                            actions.extend(column_cells(ui, table, bands.clone()));
                        });
                    if column == 0 {
                        let rect = shown.response.rect;
                        let lines = bands.len() + 1;
                        first_column = Some((rect.width(), rect.height() / lines as f32));
                    }
                }
            });
        });
    // A new grid's first pass only measures and is drawn again: its sizes are not yet the
    // columns' own (measured 250 instead of 430 points), and more columns than fit followed.
    let settled = !ui.ctx().will_discard();
    if let (Some(size), true) = (first_column, settled) {
        ui.data_mut(|data| data.insert_temp(size_id, size));
    }
    actions
}

/// One column: the titles, then the rows of `bands`.
fn column_cells(ui: &mut Ui, table: &Table, bands: std::ops::Range<usize>) -> Vec<TableAction> {
    let mut actions = Vec::new();
    ui.label(texts::BAND);
    let header_ids: Vec<(Leaf, Id)> = COLUMNS
        .iter()
        .map(|leaf| (*leaf, ui.label(header(table, *leaf)).id))
        .collect();
    ui.end_row();
    for band in bands {
        actions.extend(row_cells(ui, table, &header_ids, band, &table.rows[band]));
        ui.end_row();
    }
    actions
}

/// How many bands fit below the titles in `height`, at least one.
fn rows_per_column(row_height: f32, height: f32) -> usize {
    let lines = (height / row_height).floor() as usize;
    lines.saturating_sub(1).max(1)
}

/// How many columns `rows` bands take: as many as they need at `per_column` each, but no more
/// than `fitting` beside each other (the rest then scrolls), and at least one.
fn column_count(rows: usize, per_column: usize, fitting: usize) -> usize {
    rows.div_ceil(per_column.max(1)).min(fitting).max(1)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_column_holds_the_rows_that_fit_below_the_titles() {
        assert_eq!(rows_per_column(30.0, 210.0), 6);
        assert_eq!(
            rows_per_column(30.0, 160.0),
            4,
            "a lower window holds fewer"
        );
        assert_eq!(rows_per_column(30.0, 20.0), 1, "at least one");
    }

    /// Ten bands with six to a column take two columns where two fit beside each other; where
    /// only one fits, the one column scrolls.
    #[test]
    fn bands_overflow_into_as_many_columns_as_fit() {
        assert_eq!(column_count(10, 6, 3), 2);
        assert_eq!(column_count(10, 6, 1), 1);
        assert_eq!(column_count(20, 6, 2), 2);
        assert_eq!(column_count(5, 6, 3), 1);
        assert_eq!(
            column_count(10, 6, 0),
            1,
            "a narrow window still shows one column"
        );
    }
}
