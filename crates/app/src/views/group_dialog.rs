//! Renaming or deleting a group, opened from its strip's name.

use eframe::egui::{Id, Modal, TextEdit, Ui};

use crate::texts;

const MODAL_ID: &str = "group_dialog";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupDialogAction {
    None,
    /// Closed; the name in the draft is kept.
    Close,
    Delete,
}

pub fn show(ui: &mut Ui, name: &mut String) -> GroupDialogAction {
    let modal = Modal::new(Id::new(MODAL_ID)).show(ui.ctx(), |ui| contents(ui, name));
    if modal.should_close() {
        return GroupDialogAction::Close;
    }
    modal.inner
}

fn contents(ui: &mut Ui, name: &mut String) -> GroupDialogAction {
    ui.heading(texts::GROUP_DIALOG_TITLE);
    let label = ui.label(texts::GROUP_NAME);
    ui.add(TextEdit::singleline(name)).labelled_by(label.id);
    ui.horizontal(|ui| {
        if ui.button(texts::CLOSE).clicked() {
            return GroupDialogAction::Close;
        }
        if ui.button(texts::DELETE_GROUP).clicked() {
            return GroupDialogAction::Delete;
        }
        GroupDialogAction::None
    })
    .inner
}
