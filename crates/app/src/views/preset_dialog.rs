//! The two preset dialogs: saving chosen settings under a name, and loading a preset with a
//! preview of what it would change. The device panel's open in a window of their own, in front
//! of the panel; the EQ window's over its own contents. They only draw and report.

use eframe::egui::{
    self, Button, Context, Grid, Id, Modal, RichText, ScrollArea, TextEdit, Ui, Vec2,
    ViewportBuilder, ViewportClass, ViewportCommand, ViewportId,
};

use super::{escape_closes, placed, Opening};
use crate::texts;

const SAVE_ID: &str = "preset_save";
const LOAD_ID: &str = "preset_load";
const LIST_HEIGHT: f32 = 320.0;
const CHANGES_GRID_ID: &str = "preset_changes";
const WINDOW_SIZE: [f32; 2] = [560.0, 520.0];
const WINDOW_MIN_SIZE: [f32; 2] = [360.0, 260.0];

/// A dialog in a window of its own (`viewport`), drawn by `body`; true as well when the user
/// closed the window (its close button or Escape).
pub fn in_window<R>(
    context: &Context,
    viewport: ViewportId,
    opening: Opening,
    title: &str,
    body: impl FnOnce(&mut Ui) -> R,
) -> (R, bool) {
    let builder = ViewportBuilder::default()
        .with_title(title)
        .with_min_inner_size(WINDOW_MIN_SIZE);
    let builder = placed(builder, opening, Vec2::from(WINDOW_SIZE));
    // Opened from the panel, which has the focus: without it the new window can open behind.
    if opening != Opening::Open {
        context.send_viewport_cmd_to(viewport, ViewportCommand::Focus);
    }
    // egui takes an FnMut, but draws an immediate viewport exactly once per call.
    let mut body = Some(body);
    context.show_viewport_immediate(viewport, builder, |ui, class| {
        let body = body.take().expect("an immediate viewport is drawn once");
        let result = match class {
            ViewportClass::EmbeddedWindow => body(ui),
            _ => egui::CentralPanel::default().show(ui, body).inner,
        };
        let escape = escape_closes(ui, Id::unique(title).with(viewport));
        let close_requested = ui.input(|input| input.viewport().close_requested());
        (result, escape || close_requested)
    })
}

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

/// `rows` empty: the preset holds a fixed set (an EQ), only its name is asked. `replacing`: a
/// preset of that name already exists for the same model or band count, and saving replaces it.
pub fn save(
    ui: &mut Ui,
    title: &str,
    rows: &[SaveRow],
    name: &mut String,
    replacing: bool,
) -> SaveAction {
    let modal = Modal::new(Id::unique(SAVE_ID))
        .show(ui.ctx(), |ui| save_body(ui, title, rows, name, replacing));
    if modal.should_close() {
        return SaveAction::Cancel;
    }
    modal.inner
}

/// The save dialog's contents, wherever it is shown.
pub fn save_body(
    ui: &mut Ui,
    title: &str,
    rows: &[SaveRow],
    name: &mut String,
    replacing: bool,
) -> SaveAction {
    {
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
        if replacing {
            ui.colored_label(ui.visuals().warn_fg_color, texts::PRESET_REPLACES);
        }
        let save = match replacing {
            true => texts::REPLACE_PRESET,
            false => texts::SAVE,
        };
        ui.horizontal(|ui| {
            if ui.add_enabled(savable, Button::new(save)).clicked() {
                action = SaveAction::Save;
            }
            if ui.button(texts::CANCEL).clicked() {
                action = SaveAction::Cancel;
            }
        });
        action
    }
}

/// One setting a loaded preset would write: what it is now and what it becomes.
pub struct ComparedChange {
    pub label: String,
    pub now: String,
    pub written: String,
}

/// What loading the selected preset would write.
#[derive(Clone, Copy)]
pub enum Changes<'a> {
    /// One line each (an EQ preset's bands).
    Lines(&'a [String]),
    /// Side by side, now and to be written.
    Compared(&'a [ComparedChange]),
}

impl Changes<'_> {
    fn is_empty(self) -> bool {
        match self {
            Changes::Lines(lines) => lines.is_empty(),
            Changes::Compared(changes) => changes.is_empty(),
        }
    }
}

pub struct LoadView<'a> {
    pub title: &'a str,
    /// "Name (product)" per preset.
    pub presets: &'a [String],
    pub selected: Option<usize>,
    pub changes: Changes<'a>,
    pub skipped: &'a [String],
    /// A caution above the buttons (a crossover would lose its split).
    pub warning: Option<&'a str>,
    /// Why the speaker would refuse one of the changes; nothing is written then.
    pub refusal: Option<&'a str>,
    /// The selected preset's name while its deletion is being confirmed.
    pub deleting: Option<&'a str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadAction {
    None,
    Select(usize),
    Write,
    /// Asks before deleting.
    Delete,
    ConfirmDelete,
    CancelDelete,
    Close,
}

pub fn load(ui: &mut Ui, view: &LoadView) -> LoadAction {
    let modal = Modal::new(Id::unique(LOAD_ID)).show(ui.ctx(), |ui| load_body(ui, view));
    if modal.should_close() {
        return LoadAction::Close;
    }
    modal.inner
}

/// The load dialog's contents, wherever it is shown.
pub fn load_body(ui: &mut Ui, view: &LoadView) -> LoadAction {
    {
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
                .show(ui, |ui| changes(ui, view.changes));
            if !view.skipped.is_empty() {
                ui.weak(texts::preset_skipped(view.skipped));
            }
        }
        if let Some(warning) = view.warning {
            ui.colored_label(ui.visuals().warn_fg_color, warning);
        }
        if let Some(refusal) = view.refusal {
            ui.colored_label(ui.visuals().error_fg_color, refusal);
        }
        // A deleted preset is gone for good: the delete is asked once more.
        if let Some(name) = view.deleting {
            ui.label(texts::delete_question(name));
            ui.horizontal(|ui| {
                if ui.button(texts::DELETE_PRESET).clicked() {
                    action = LoadAction::ConfirmDelete;
                }
                if ui.button(texts::CANCEL).clicked() {
                    action = LoadAction::CancelDelete;
                }
            });
            return action;
        }
        ui.horizontal(|ui| {
            let writable =
                view.selected.is_some() && !view.changes.is_empty() && view.refusal.is_none();
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
    }
}

fn changes(ui: &mut Ui, changes: Changes) {
    match changes {
        Changes::Lines(lines) => {
            for line in lines {
                ui.label(line);
            }
        }
        Changes::Compared(compared) => {
            Grid::new(CHANGES_GRID_ID).striped(true).show(ui, |ui| {
                ui.label(RichText::new(texts::PRESET_SETTING).strong());
                ui.label(RichText::new(texts::PRESET_NOW).strong());
                ui.label(RichText::new(texts::PRESET_WRITTEN).strong());
                ui.end_row();
                for change in compared {
                    ui.label(&change.label);
                    ui.label(&change.now);
                    ui.label(RichText::new(&change.written).strong());
                    ui.end_row();
                }
            });
        }
    }
}
