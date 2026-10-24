//! The "Alle Werte" window: every leaf of the device schema with its value, read-only, in a
//! window of its own like the EQ and the panel. The only thing it can ask the worker for is a
//! fresh read.

use eframe::egui::{
    self, Context, Id, Label, ScrollArea, TextEdit, Ui, ViewportBuilder, ViewportClass, ViewportId,
};
use neumann_control_core::catalog::Catalog;
use neumann_control_core::speaker::Command;

use super::{escape_closes, WindowPlace};
use crate::all_values::{rows, Row};
use crate::state::{AllValuesState, SpeakerKey, SpeakerState};
use crate::texts;
use eframe::egui::Vec2;

const WINDOW_ID: &str = "all_values";
const DEFAULT_WINDOW_SIZE: [f32; 2] = [920.0, 480.0];
const PATH_WIDTH: f32 = 250.0;
const VALUE_WIDTH: f32 = 280.0;
const RANGE_WIDTH: f32 = 180.0;
const KNOWN_WIDTH: f32 = 70.0;
const FILTER_WIDTH: f32 = 220.0;

/// Draws the window while the speaker's state is not idle and closes it (back to idle) when
/// the user does: its close button, or Escape outside the filter. Returns the commands the user
/// asked for.
pub fn show(
    context: &Context,
    speaker: &mut SpeakerState,
    catalog: &Catalog,
    place: Option<WindowPlace>,
) -> Vec<Command> {
    let viewport = viewport_of(&speaker.key);
    let size = place.map_or(Vec2::from(DEFAULT_WINDOW_SIZE), |place| place.size);
    let mut builder = ViewportBuilder::default()
        .with_title(texts::all_values_title(&speaker.display_name()))
        .with_inner_size(size);
    if let Some(position) = place.and_then(|place| place.position) {
        builder = builder.with_position(position);
    }
    let mut commands = Vec::new();
    let close = context.show_viewport_immediate(viewport, builder, |ui, class| {
        ui.push_id(viewport, |ui| match class {
            ViewportClass::EmbeddedWindow => contents(ui, speaker, catalog, &mut commands),
            _ => {
                egui::CentralPanel::default()
                    .show(ui, |ui| contents(ui, speaker, catalog, &mut commands));
            }
        });
        let escape = escape_closes(ui, Id::unique(WINDOW_ID).with(viewport));
        escape || ui.input(|input| input.viewport().close_requested())
    });
    if close {
        speaker.close_all_values();
    }
    commands
}

fn contents(
    ui: &mut Ui,
    speaker: &mut SpeakerState,
    catalog: &Catalog,
    commands: &mut Vec<Command>,
) {
    ui.horizontal(|ui| {
        let label = ui.label(texts::ALL_VALUES_FILTER_LABEL);
        ui.add(TextEdit::singleline(&mut speaker.all_values_filter).desired_width(FILTER_WIDTH))
            .labelled_by(label.id);
        if ui
            .add_enabled(
                speaker.can_read_all_values(),
                eframe::egui::Button::new(texts::ALL_VALUES_RELOAD),
            )
            .clicked()
        {
            commands.push(speaker.read_all_values());
        }
    });
    ui.separator();
    match &speaker.all_values {
        AllValuesState::Idle => {}
        AllValuesState::Loading => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(texts::ALL_VALUES_LOADING);
            });
        }
        AllValuesState::Failed(_) => {
            ui.colored_label(ui.visuals().error_fg_color, texts::ALL_VALUES_FAILED);
        }
        AllValuesState::Loaded(reports) => {
            let shown = rows(reports, &speaker.all_values_filter, catalog);
            ui.weak(texts::all_values_count(shown.len(), reports.len()));
            table(ui, &shown);
        }
    }
}

fn table(ui: &mut Ui, shown: &[Row<'_>]) {
    header(ui);
    let row_height = ui.spacing().interact_size.y;
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(ui, row_height, shown.len(), |ui, range| {
            for row in &shown[range] {
                row_cells(ui, row);
            }
        });
}

fn header(ui: &mut Ui) {
    ui.horizontal(|ui| {
        cell(ui, PATH_WIDTH, texts::ALL_VALUES_PATH_COLUMN.get());
        cell(ui, VALUE_WIDTH, texts::ALL_VALUES_VALUE_COLUMN.get());
        cell(ui, RANGE_WIDTH, texts::ALL_VALUES_RANGE_COLUMN.get());
        cell(ui, KNOWN_WIDTH, texts::ALL_VALUES_KNOWN_COLUMN.get());
    });
}

fn row_cells(ui: &mut Ui, row: &Row<'_>) {
    ui.horizontal(|ui| {
        cell(ui, PATH_WIDTH, &row.path);
        cell(ui, VALUE_WIDTH, &row.value);
        cell(ui, RANGE_WIDTH, &row.range);
        let known = if row.known {
            texts::ALL_VALUES_KNOWN.get()
        } else {
            ""
        };
        cell(ui, KNOWN_WIDTH, known);
        if ui.button(texts::ALL_VALUES_COPY).clicked() {
            ui.ctx().copy_text(row.copy_text());
        }
    });
}

/// A label in a fixed-width column; what does not fit is cut, the cell's text is on hover.
fn cell(ui: &mut Ui, width: f32, text: &str) {
    let height = ui.spacing().interact_size.y;
    ui.add_sized([width, height], Label::new(text).truncate())
        .on_hover_text(text);
}

/// The window's viewport for a speaker, to read where the user left it.
pub fn viewport_of(key: &SpeakerKey) -> ViewportId {
    ViewportId::from_hash_of(Id::unique((WINDOW_ID, key.clone())))
}
