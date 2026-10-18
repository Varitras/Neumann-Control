//! The confirmation before a card's speaker is removed and forgotten.

use eframe::egui::{Id, Modal, Ui};

use crate::texts;

const MODAL_ID: &str = "remove_speaker";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoveAction {
    None,
    Confirm,
    Cancel,
}

/// `name` is the card's display name, so the user sees which speaker goes.
pub fn show(ui: &mut Ui, name: &str) -> RemoveAction {
    let modal = Modal::new(Id::new(MODAL_ID)).show(ui.ctx(), |ui| contents(ui, name));
    if modal.should_close() {
        return RemoveAction::Cancel;
    }
    modal.inner
}

fn contents(ui: &mut Ui, name: &str) -> RemoveAction {
    ui.heading(texts::REMOVE_TITLE);
    ui.label(name);
    ui.horizontal(|ui| {
        if ui.button(texts::REMOVE).clicked() {
            return RemoveAction::Confirm;
        }
        if ui.button(texts::CANCEL).clicked() {
            return RemoveAction::Cancel;
        }
        RemoveAction::None
    })
    .inner
}
