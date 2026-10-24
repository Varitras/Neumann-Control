//! Renaming, colouring or deleting a group, opened from its strip's name.

use eframe::egui::{Color32, Id, Modal, TextEdit, Ui};
use neumann_control_core::settings::ColorChoice;

use super::color_choice;
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

/// The group's name and colour as edited, the palette colour an own one starts from.
pub struct GroupDraftView<'a> {
    pub name: &'a mut String,
    pub color: &'a mut ColorChoice,
    pub palette: Color32,
    /// The delete is being confirmed. A group goes with its members' places and links.
    pub deleting: bool,
}

pub fn show(ui: &mut Ui, draft: GroupDraftView) -> GroupDialogAction {
    let modal = Modal::new(Id::unique(MODAL_ID)).show(ui.ctx(), |ui| contents(ui, draft));
    if modal.should_close() {
        return GroupDialogAction::Close;
    }
    modal.inner
}

fn contents(ui: &mut Ui, draft: GroupDraftView) -> GroupDialogAction {
    let GroupDraftView {
        name,
        color,
        palette,
        deleting,
    } = draft;
    ui.heading(texts::GROUP_DIALOG_TITLE);
    let label = ui.label(texts::GROUP_NAME);
    ui.add(TextEdit::singleline(name)).labelled_by(label.id);
    color_choice::edit(ui, texts::COLOR_DEFAULT.get(), color, palette);
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
