//! The history window behind the toolbar's "Verlauf": the writes of this session, newest on top,
//! each with "Zurück"; and the question before taking back values that changed since.

use eframe::egui::{Button, Id, Key, KeyboardShortcut, Modal, Modifiers, RichText, ScrollArea, Ui};

use crate::texts;

const MODAL_ID: &str = "history";
const QUESTION_ID: &str = "history_question";
const LIST_HEIGHT: f32 = 420.0;
const LIST_WIDTH: f32 = 520.0;

/// One entry as the window shows it, worded by the app.
pub struct HistoryRow {
    pub id: u64,
    pub time: String,
    pub text: String,
    /// Every change of an entry with several, shown on hover.
    pub details: Vec<String>,
    /// Whether "Zurück" can take something back; otherwise `reason` says why not.
    pub open: bool,
    pub reason: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryAction {
    None,
    Close,
    TakeBack(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuestionAction {
    None,
    Confirm,
    Cancel,
}

/// `row_at` words the row at a place, newest first; only the rows in sight are asked for.
pub fn show(
    ui: &mut Ui,
    count: usize,
    row_at: &dyn Fn(usize) -> Option<HistoryRow>,
    notice: Option<&str>,
) -> HistoryAction {
    let modal =
        Modal::new(Id::new(MODAL_ID)).show(ui.ctx(), |ui| contents(ui, count, row_at, notice));
    if modal.should_close() {
        return HistoryAction::Close;
    }
    modal.inner
}

fn contents(
    ui: &mut Ui,
    count: usize,
    row_at: &dyn Fn(usize) -> Option<HistoryRow>,
    notice: Option<&str>,
) -> HistoryAction {
    let mut action = HistoryAction::None;
    ui.heading(texts::HISTORY_TITLE);
    if let Some(notice) = notice {
        ui.colored_label(ui.visuals().warn_fg_color, notice);
    }
    if count == 0 {
        ui.weak(texts::NO_HISTORY);
    }
    let row_height = ui.spacing().interact_size.y;
    ScrollArea::vertical()
        .max_height(LIST_HEIGHT)
        .min_scrolled_width(LIST_WIDTH)
        .show_rows(ui, row_height, count, |ui, places| {
            for row in places.filter_map(row_at) {
                if let Some(id) = row_line(ui, &row) {
                    action = HistoryAction::TakeBack(id);
                }
            }
        });
    ui.separator();
    if ui.button(texts::CLOSE).clicked() {
        action = HistoryAction::Close;
    }
    action
}

/// Returns the entry when its "Zurück" was clicked.
fn row_line(ui: &mut Ui, row: &HistoryRow) -> Option<u64> {
    ui.horizontal(|ui| {
        let button = ui.add_enabled(row.open, Button::new(texts::TAKE_BACK));
        let button = match &row.reason {
            Some(reason) => button.on_disabled_hover_text(reason),
            None => button,
        };
        ui.label(RichText::new(&row.time).monospace().weak());
        let text = if row.open {
            RichText::new(&row.text)
        } else {
            RichText::new(&row.text).weak()
        };
        let label = ui.label(text);
        if !row.details.is_empty() {
            label.on_hover_text(row.details.join("\n"));
        }
        button.clicked().then_some(row.id)
    })
    .inner
}

/// "Seitdem geändert": the changes whose values are no longer the written ones.
/// Ctrl+Z, in the main window and in the EQ windows.
pub const UNDO_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);

pub fn question(ui: &mut Ui, lines: &[String]) -> QuestionAction {
    let modal = Modal::new(Id::new(QUESTION_ID)).show(ui.ctx(), |ui| {
        ui.heading(texts::CHANGED_SINCE);
        for line in lines {
            ui.label(line);
        }
        ui.label(texts::REVERT_ANYWAY);
        ui.horizontal(|ui| {
            if ui.button(texts::REVERT).clicked() {
                return QuestionAction::Confirm;
            }
            if ui.button(texts::CANCEL).clicked() {
                return QuestionAction::Cancel;
            }
            QuestionAction::None
        })
        .inner
    });
    if modal.should_close() {
        return QuestionAction::Cancel;
    }
    modal.inner
}
