//! The two preset dialogs of the device panel: saving chosen settings under a name, and loading
//! a preset with a preview of what it would change. They only draw and report.

use eframe::egui::{Button, Id, Modal, ScrollArea, TextEdit, Ui};

use crate::texts;

const SAVE_ID: &str = "preset_save";
const LOAD_ID: &str = "preset_load";
const LIST_HEIGHT: f32 = 320.0;

/// A setting offered for the preset: its name, current value and whether it is chosen.
pub struct SaveRow {
    pub label: String,
    pub value: String,
    pub chosen: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveAction {
    None,
    Toggle(usize, bool),
    Save,
    Cancel,
}

/// `rows` empty: the preset holds a fixed set (an EQ), only its name is asked.
pub fn save(ui: &mut Ui, title: &str, rows: &[SaveRow], name: &mut String) -> SaveAction {
    let modal = Modal::new(Id::new(SAVE_ID)).show(ui.ctx(), |ui| {
        ui.heading(title);
        let label = ui.label(texts::PRESET_NAME);
        ui.add(TextEdit::singleline(name)).labelled_by(label.id);
        let mut action = SaveAction::None;
        ScrollArea::vertical()
            .max_height(LIST_HEIGHT)
            .show(ui, |ui| {
                for (place, row) in rows.iter().enumerate() {
                    let mut chosen = row.chosen;
                    let text = texts::preset_row(&row.label, &row.value);
                    if ui.checkbox(&mut chosen, text).changed() {
                        action = SaveAction::Toggle(place, chosen);
                    }
                }
            });
        let something = rows.is_empty() || rows.iter().any(|row| row.chosen);
        let savable = !name.trim().is_empty() && something;
        ui.horizontal(|ui| {
            if ui.add_enabled(savable, Button::new(texts::SAVE)).clicked() {
                action = SaveAction::Save;
            }
            if ui.button(texts::CANCEL).clicked() {
                action = SaveAction::Cancel;
            }
        });
        action
    });
    if modal.should_close() {
        return SaveAction::Cancel;
    }
    modal.inner
}

pub struct LoadView<'a> {
    pub title: &'a str,
    /// "Name (product)" per preset.
    pub presets: &'a [String],
    pub selected: Option<usize>,
    /// "Setting: current → preset" per write.
    pub changes: &'a [String],
    pub skipped: &'a [String],
    /// A caution above the buttons (a crossover would lose its split).
    pub warning: Option<&'a str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadAction {
    None,
    Select(usize),
    Write,
    Delete,
    Close,
}

pub fn load(ui: &mut Ui, view: &LoadView) -> LoadAction {
    let modal = Modal::new(Id::new(LOAD_ID)).show(ui.ctx(), |ui| {
        ui.heading(view.title);
        let mut action = LoadAction::None;
        if view.presets.is_empty() {
            ui.weak(texts::NO_PRESETS);
        }
        for (place, preset) in view.presets.iter().enumerate() {
            if ui
                .selectable_label(view.selected == Some(place), preset)
                .clicked()
            {
                action = LoadAction::Select(place);
            }
        }
        if view.selected.is_some() {
            ui.separator();
            if view.changes.is_empty() {
                ui.weak(texts::PRESET_UNCHANGED);
            }
            ScrollArea::vertical()
                .max_height(LIST_HEIGHT)
                .show(ui, |ui| {
                    for change in view.changes {
                        ui.label(change);
                    }
                });
            if !view.skipped.is_empty() {
                ui.weak(texts::preset_skipped(view.skipped));
            }
        }
        if let Some(warning) = view.warning {
            ui.colored_label(ui.visuals().warn_fg_color, warning);
        }
        ui.horizontal(|ui| {
            let writable = view.selected.is_some() && !view.changes.is_empty();
            if ui
                .add_enabled(writable, Button::new(texts::WRITE_PRESET))
                .clicked()
            {
                action = LoadAction::Write;
            }
            let deletable = view.selected.is_some();
            if ui
                .add_enabled(deletable, Button::new(texts::DELETE_PRESET))
                .clicked()
            {
                action = LoadAction::Delete;
            }
            if ui.button(texts::CLOSE).clicked() {
                action = LoadAction::Close;
            }
        });
        action
    });
    if modal.should_close() {
        return LoadAction::Close;
    }
    modal.inner
}
