//! Renaming or deleting a group, opened from its strip's name.

use eframe::egui::{Id, Modal, TextEdit, Ui};

use crate::texts;

const MODAL_ID: &str = "group_dialog";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupDialogAction {
    None,
    /// Closed; the name in the draft is kept.
    Close,
    /// Asks before deleting.
    Delete,
    ConfirmDelete,
    CancelDelete,
}

/// `deleting`: the delete is being confirmed. A group goes with its members' places and links.
pub fn show(ui: &mut Ui, name: &mut String, deleting: bool) -> GroupDialogAction {
    let modal = Modal::new(Id::unique(MODAL_ID)).show(ui.ctx(), |ui| contents(ui, name, deleting));
    if modal.should_close() {
        return GroupDialogAction::Close;
    }
    modal.inner
}

fn contents(ui: &mut Ui, name: &mut String, deleting: bool) -> GroupDialogAction {
    ui.heading(texts::GROUP_DIALOG_TITLE);
    let label = ui.label(texts::GROUP_NAME);
    ui.add(TextEdit::singleline(name)).labelled_by(label.id);
    if deleting {
        ui.label(texts::delete_question(name.trim()));
        return ui
            .horizontal(|ui| {
                if ui.button(texts::DELETE_GROUP).clicked() {
                    return GroupDialogAction::ConfirmDelete;
                }
                if ui.button(texts::CANCEL).clicked() {
                    return GroupDialogAction::CancelDelete;
                }
                GroupDialogAction::None
            })
            .inner;
    }
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
