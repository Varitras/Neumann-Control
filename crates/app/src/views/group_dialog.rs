//! Renaming, colouring, filling or deleting a group, opened from its strip's name or
//! **Details**, from **Delete group…** in its context menu, and right after **+ Group**.

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

/// A speaker the group could have, as its dialog lists it.
pub struct MemberChoice {
    pub name: String,
    /// The other group it is in; checking it moves it from there.
    pub elsewhere: Option<String>,
    pub checked: bool,
}

/// The group's name, colour and members as edited, the palette colour an own one starts from.
pub struct GroupDraftView<'a> {
    pub name: &'a mut String,
    pub color: &'a mut ColorChoice,
    pub palette: Color32,
    pub members: &'a mut [MemberChoice],
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
        members,
        deleting,
    } = draft;
    ui.heading(texts::GROUP_DIALOG_TITLE);
    let label = ui.label(texts::GROUP_NAME);
    ui.add(TextEdit::singleline(name)).labelled_by(label.id);
    color_choice::edit(ui, texts::COLOR_DEFAULT.get(), color, palette);
    member_checks(ui, members);
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

fn member_checks(ui: &mut Ui, members: &mut [MemberChoice]) {
    if members.is_empty() {
        return;
    }
    ui.label(texts::GROUP_MEMBERS);
    for member in members {
        let response = ui.checkbox(&mut member.checked, &member.name);
        if let Some(group) = &member.elsewhere {
            response.on_hover_text(texts::in_other_group(group));
        }
    }
}
