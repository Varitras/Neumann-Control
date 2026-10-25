//! The EQ window of one speaker, an OS window of its own (embedded as an inner window where the
//! backend has only one): output and container choice, the graph and the band table.

use eframe::egui::{
    self, vec2, Button, Color32, Context, Id, Modal, RichText, ScrollArea, Shape, Stroke, Ui,
    ViewportBuilder, ViewportClass, ViewportId,
};

use super::eq_graph::{self, Graph, GraphAction};
use super::eq_table::{self, Table, TableAction};
use super::escape_closes;
use super::history::UNDO_SHORTCUT;
use super::preset_dialog::{self, Changes, ComparedChange, LoadAction, LoadView, SaveAction};
use super::wheel;
use super::{placed, Opening};
use crate::eq::{self, ResetKind};
use crate::texts;
use eframe::egui::Vec2;

/// Keys the window's own memory (Escape).
const EQ_WINDOW_ID: &str = "eq_window";
pub(crate) const EQ_WINDOW_SIZE: [f32; 2] = [900.0, 640.0];
const EQ_WINDOW_MIN_SIZE: [f32; 2] = [640.0, 480.0];
const LEGEND_SWATCH: f32 = 10.0;
const SUM_SWATCH_LENGTH: f32 = 18.0;
const SUM_SWATCH_WIDTH: f32 = 2.5;
const SUM_SWATCH_DASH: f32 = 3.0;
const IMPORT_ID: &str = "eq_import";
const IMPORT_LIST_HEIGHT: f32 = 240.0;

pub struct EqView<'a> {
    pub viewport: ViewportId,
    pub opening: Opening,
    pub title: String,
    /// The outputs to choose from; one output needs no choice.
    pub outputs: &'a [String],
    pub output: usize,
    /// The containers of the chosen output, with their curve colours.
    pub tabs: &'a [(String, Color32)],
    pub container: usize,
    pub graph: Graph<'a>,
    /// `None` while the container's bands are not (or not sensibly) reported.
    pub table: Option<Table<'a>>,
    /// Why nothing can be written right now, or why the last write was refused.
    pub notice: Option<String>,
    /// The reset of the chosen container; `None` while it cannot be written, and on a crossover
    /// while crossovers are protected.
    pub reset: Option<ResetOffer>,
    /// The history line Undo would take back; `None` disables it.
    pub undo: Option<String>,
    /// "Changed since" for the entry being undone: the changes another client made meanwhile.
    pub question: Option<Vec<String>>,
    /// Other speakers with the same output: name and whether its sum is compared.
    pub candidates: &'a [(String, bool)],
    /// The compared curves that are drawn, for the legend: name and colour.
    pub compared: &'a [(String, Color32)],
    /// The sums' legend: name and whether it is drawn dashed.
    pub sums: &'a [(String, bool)],
    /// Whether the chosen container can be saved as or loaded from a preset, or imported into.
    pub presets: bool,
    pub preset_dialog: Option<PresetDialogView<'a>>,
    /// Whether the chosen container can be exported (its bands are reported).
    pub exportable: bool,
    pub import: Option<ImportView<'a>>,
    /// What the last export did, line by line.
    pub file_report: &'a [String],
}

/// The preview of a filter file before it is written.
pub struct ImportView<'a> {
    pub file: &'a str,
    /// The bands that would change, also while `refusal` says why nothing is written.
    pub changes: &'a [String],
    /// What the file holds that is not taken over.
    pub notes: &'a [String],
    /// Why the file cannot be written; Write is then disabled.
    pub refusal: Option<&'a str>,
    /// The container is a crossover (protection off): writing warns.
    pub crossover: bool,
}

pub enum PresetDialogView<'a> {
    Save {
        name: &'a str,
        /// A preset of that name and band count exists and would be replaced.
        replacing: bool,
    },
    Load {
        /// The presets that fit the container.
        names: &'a [String],
        selected: Option<usize>,
        /// Each band's value that would change, now and as written.
        changes: &'a [ComparedChange],
        /// The container is a crossover (protection off): loading warns.
        crossover: bool,
        /// The selected preset's name while its deletion is being confirmed.
        deleting: Option<&'a str>,
    },
}

pub struct ResetOffer {
    pub container: String,
    /// The reset being asked about; its question is shown instead of the buttons.
    pub asking: Option<ResetKind>,
    /// The container is a crossover (offered only with the protection off): the question warns.
    pub crossover: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EqAction {
    Close,
    Output(usize),
    Container(usize),
    Graph(GraphAction),
    Table(TableAction),
    ResetAsk(ResetKind),
    ResetConfirm,
    ResetCancel,
    Undo,
    QuestionConfirm,
    QuestionCancel,
    /// The candidate at this place is compared or no longer.
    Compare(usize, bool),
    PresetSave,
    PresetLoad,
    PresetName(String),
    PresetConfirmSave,
    /// The place among the fitting presets.
    PresetSelect(usize),
    PresetWrite,
    PresetDelete,
    PresetConfirmDelete,
    PresetCancelDelete,
    PresetClose,
    Import,
    ImportWrite,
    ImportCancel,
    Export,
}

pub fn show(context: &Context, view: &EqView) -> Vec<EqAction> {
    let builder = ViewportBuilder::default()
        .with_title(&view.title)
        .with_min_inner_size(EQ_WINDOW_MIN_SIZE);
    let builder = placed(builder, view.opening, Vec2::from(EQ_WINDOW_SIZE));
    context.show_viewport_immediate(view.viewport, builder, |ui, class| {
        super::note_repaint_causes(ui);
        wheel::track_burst(ui.ctx());
        let mut actions = match class {
            ViewportClass::EmbeddedWindow => contents(ui, view),
            _ => {
                egui::CentralPanel::default()
                    .show(ui, |ui| contents(ui, view))
                    .inner
            }
        };
        let close_requested = ui.input(|input| input.viewport().close_requested());
        let escape = escape_closes(ui, Id::unique(EQ_WINDOW_ID).with(view.viewport));
        if close_requested || escape {
            actions.push(EqAction::Close);
        }
        let typing = ui.memory(|memory| memory.focused().is_some());
        if !typing && ui.input_mut(|input| input.consume_shortcut(&UNDO_SHORTCUT)) {
            actions.push(EqAction::Undo);
        }
        actions
    })
}

fn contents(ui: &mut Ui, view: &EqView) -> Vec<EqAction> {
    let mut actions = Vec::new();
    if view.outputs.len() > 1 {
        ui.horizontal(|ui| {
            for (index, output) in view.outputs.iter().enumerate() {
                if ui.selectable_label(index == view.output, output).clicked() {
                    actions.push(EqAction::Output(index));
                }
            }
        });
    }
    ui.horizontal(|ui| {
        for (index, (label, color)) in view.tabs.iter().enumerate() {
            let (swatch, _) =
                ui.allocate_exact_size(vec2(LEGEND_SWATCH, LEGEND_SWATCH), egui::Sense::hover());
            ui.painter().rect_filled(swatch, 2.0, *color);
            if ui
                .selectable_label(index == view.container, label)
                .clicked()
            {
                actions.push(EqAction::Container(index));
            }
        }
        for (name, dashed) in view.sums {
            sum_swatch(ui, *dashed);
            ui.label(RichText::new(name).strong());
        }
        for (name, color) in view.compared {
            let (swatch, _) =
                ui.allocate_exact_size(vec2(LEGEND_SWATCH, LEGEND_SWATCH), egui::Sense::hover());
            ui.painter().rect_filled(swatch, 2.0, *color);
            ui.label(texts::compared_curve(name));
        }
    });
    actions.extend(compare_row(ui, view.candidates));
    actions.extend(
        eq_graph::show(ui, &view.graph)
            .into_iter()
            .map(EqAction::Graph),
    );
    ui.weak(texts::EQ_COMPUTED);
    if let Some(notice) = &view.notice {
        ui.colored_label(ui.visuals().warn_fg_color, notice);
    }
    actions.extend(edit_row(ui, view.reset.as_ref(), view.undo.as_deref()));
    ui.horizontal(|ui| {
        if view.presets && ui.button(texts::EQ_PRESET_SAVE).clicked() {
            actions.push(EqAction::PresetSave);
        }
        if view.presets && ui.button(texts::EQ_PRESET_LOAD).clicked() {
            actions.push(EqAction::PresetLoad);
        }
        if view.presets && ui.button(texts::EQ_IMPORT).clicked() {
            actions.push(EqAction::Import);
        }
        if view.exportable && ui.button(texts::EQ_EXPORT).clicked() {
            actions.push(EqAction::Export);
        }
    });
    for line in view.file_report {
        ui.label(line);
    }
    if let Some(dialog) = &view.preset_dialog {
        actions.extend(preset_dialog(ui, dialog));
    }
    if let Some(import) = &view.import {
        actions.extend(import_preview(ui, import));
    }
    if let Some(lines) = &view.question {
        actions.extend(question(ui, lines));
    }
    ui.separator();
    match &view.table {
        Some(table) => actions.extend(eq_table::show(ui, table).into_iter().map(EqAction::Table)),
        None => {
            ui.weak(texts::EQ_NOT_REPORTED);
        }
    }
    ui.separator();
    if ui.button(texts::CLOSE).clicked() {
        actions.push(EqAction::Close);
    }
    actions
}

fn preset_dialog(ui: &mut Ui, dialog: &PresetDialogView) -> Option<EqAction> {
    match dialog {
        PresetDialogView::Save { name, replacing } => {
            let mut typed = (*name).to_owned();
            let title = texts::EQ_PRESET_SAVE_TITLE.get();
            let action = preset_dialog::save(ui, title, &[], &mut typed, *replacing);
            if typed != *name {
                return Some(EqAction::PresetName(typed));
            }
            match action {
                SaveAction::Save => Some(EqAction::PresetConfirmSave),
                SaveAction::Cancel => Some(EqAction::PresetClose),
                SaveAction::None | SaveAction::Toggle(..) => None,
            }
        }
        PresetDialogView::Load {
            names,
            selected,
            changes,
            crossover,
            deleting,
        } => {
            let view = LoadView {
                title: texts::EQ_PRESET_LOAD_TITLE.get(),
                presets: names,
                selected: *selected,
                changes: Changes::Compared(changes),
                skipped: &[],
                warning: crossover.then(|| texts::CROSSOVER_WARNING.get()),
                refusal: None,
                deleting: *deleting,
            };
            match preset_dialog::load(ui, &view) {
                LoadAction::None => None,
                LoadAction::Select(place) => Some(EqAction::PresetSelect(place)),
                LoadAction::Write => Some(EqAction::PresetWrite),
                LoadAction::Delete => Some(EqAction::PresetDelete),
                LoadAction::ConfirmDelete => Some(EqAction::PresetConfirmDelete),
                LoadAction::CancelDelete => Some(EqAction::PresetCancelDelete),
                LoadAction::Close => Some(EqAction::PresetClose),
            }
        }
    }
}

fn import_preview(ui: &mut Ui, import: &ImportView) -> Option<EqAction> {
    let modal = Modal::new(Id::unique(IMPORT_ID)).show(ui.ctx(), |ui| {
        ui.heading(texts::EQ_IMPORT_TITLE);
        ui.label(texts::eq_import_from(import.file));
        if let Some(refusal) = import.refusal {
            ui.colored_label(ui.visuals().warn_fg_color, refusal);
        }
        let unchanged = import.refusal.is_none() && import.changes.is_empty();
        if unchanged {
            ui.weak(texts::IMPORT_UNCHANGED);
        }
        ScrollArea::vertical()
            .max_height(IMPORT_LIST_HEIGHT)
            .show(ui, |ui| {
                for change in import.changes {
                    ui.label(change);
                }
            });
        for note in import.notes {
            ui.weak(note);
        }
        if import.crossover {
            ui.colored_label(ui.visuals().warn_fg_color, texts::CROSSOVER_WARNING);
        }
        ui.horizontal(|ui| {
            let writable = import.refusal.is_none() && !import.changes.is_empty();
            if ui
                .add_enabled(writable, Button::new(texts::WRITE_PRESET))
                .clicked()
            {
                return Some(EqAction::ImportWrite);
            }
            ui.button(texts::CANCEL)
                .clicked()
                .then_some(EqAction::ImportCancel)
        })
        .inner
    });
    if modal.should_close() {
        return Some(EqAction::ImportCancel);
    }
    modal.inner
}

fn compare_row(ui: &mut Ui, candidates: &[(String, bool)]) -> Option<EqAction> {
    if candidates.is_empty() {
        return None;
    }
    ui.horizontal_wrapped(|ui| {
        ui.label(texts::COMPARE_WITH);
        let mut action = None;
        for (place, (name, checked)) in candidates.iter().enumerate() {
            let mut now = *checked;
            if ui.checkbox(&mut now, name).changed() {
                action = Some(EqAction::Compare(place, now));
            }
        }
        action
    })
    .inner
}

/// Reset and undo in one row; while the reset is asked about, its question takes the row.
fn edit_row(ui: &mut Ui, reset: Option<&ResetOffer>, undo: Option<&str>) -> Option<EqAction> {
    ui.horizontal(|ui| {
        if let Some((asked, kind)) = reset.and_then(|reset| Some((reset, reset.asking?))) {
            return reset_question(ui, asked, kind);
        }
        let flatten = reset.is_some() && ui.button(texts::EQ_RESET).clicked();
        let full = reset.is_some() && ui.button(texts::EQ_FULL_RESET).clicked();
        let undone = ui
            .add_enabled(undo.is_some(), Button::new(texts::UNDO))
            .clicked();
        if let Some(line) = undo {
            ui.weak(line);
        }
        if flatten {
            return Some(EqAction::ResetAsk(ResetKind::Flat));
        }
        if full {
            return Some(EqAction::ResetAsk(ResetKind::Full));
        }
        undone.then_some(EqAction::Undo)
    })
    .inner
}

fn question(ui: &mut Ui, lines: &[String]) -> Option<EqAction> {
    ui.group(|ui| {
        ui.strong(texts::CHANGED_SINCE);
        for line in lines {
            ui.label(line);
        }
        ui.label(texts::REVERT_ANYWAY);
        ui.horizontal(|ui| {
            if ui.button(texts::REVERT).clicked() {
                return Some(EqAction::QuestionConfirm);
            }
            ui.button(texts::CANCEL)
                .clicked()
                .then_some(EqAction::QuestionCancel)
        })
        .inner
    })
    .inner
}

fn reset_question(ui: &mut Ui, reset: &ResetOffer, kind: ResetKind) -> Option<EqAction> {
    let (question, confirm) = match kind {
        ResetKind::Flat => (texts::eq_reset_question(&reset.container), texts::EQ_RESET),
        ResetKind::Full => (
            texts::eq_full_reset_question(
                &reset.container,
                eq::RESET_TYPE,
                eq::RESET_FREQUENCY_HZ,
                eq::RESET_Q,
            ),
            texts::EQ_FULL_RESET,
        ),
    };
    ui.label(question);
    if reset.crossover {
        ui.colored_label(ui.visuals().warn_fg_color, texts::CROSSOVER_WARNING);
    }
    if ui.button(confirm).clicked() {
        return Some(EqAction::ResetConfirm);
    }
    ui.button(texts::CANCEL)
        .clicked()
        .then_some(EqAction::ResetCancel)
}

/// A short piece of the sum's line, solid or dashed, so the legend tells the inputs apart.
fn sum_swatch(ui: &mut Ui, dashed: bool) {
    let (rect, _) =
        ui.allocate_exact_size(vec2(SUM_SWATCH_LENGTH, LEGEND_SWATCH), egui::Sense::hover());
    let stroke = Stroke::new(SUM_SWATCH_WIDTH, ui.visuals().text_color());
    let line = [rect.left_center(), rect.right_center()];
    match dashed {
        true => {
            let dashes = Shape::dashed_line(&line, stroke, SUM_SWATCH_DASH, SUM_SWATCH_DASH);
            ui.painter().extend(dashes);
        }
        false => {
            ui.painter().line_segment(line, stroke);
        }
    }
}
