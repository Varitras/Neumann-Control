//! The "Gefundene Lautsprecher" dialog after "Suchen": a spinner while the scan runs, then one
//! row per found speaker to check. Speakers already shown are listed but not offered.

use eframe::egui::{Button, Checkbox, Grid, Id, Modal, Ui};

use crate::scan_choice::{Offer, ScanChoice};
use crate::texts;

const MODAL_ID: &str = "scan_choice";
const GRID_ID: &str = "scan_choice_rows";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanChoiceAction {
    None,
    Toggle(usize),
    Add,
    Cancel,
}

pub fn show(ui: &mut Ui, choice: &ScanChoice) -> ScanChoiceAction {
    let modal = Modal::new(Id::new(MODAL_ID)).show(ui.ctx(), |ui| contents(ui, choice));
    if modal.should_close() {
        return ScanChoiceAction::Cancel;
    }
    modal.inner
}

fn contents(ui: &mut Ui, choice: &ScanChoice) -> ScanChoiceAction {
    ui.heading(texts::SCAN_CHOICE_TITLE);
    let mut action = match choice {
        ScanChoice::Scanning => {
            ui.spinner();
            ScanChoiceAction::None
        }
        ScanChoice::Failed => {
            ui.colored_label(ui.visuals().error_fg_color, texts::SCAN_FAILED);
            ScanChoiceAction::None
        }
        ScanChoice::Offered(offers) if offers.is_empty() => {
            ui.label(texts::NOTHING_FOUND);
            ScanChoiceAction::None
        }
        ScanChoice::Offered(offers) => rows(ui, offers),
    };
    ui.horizontal(|ui| {
        if ui
            .add_enabled(choice.can_add(), Button::new(texts::ADD))
            .clicked()
        {
            action = ScanChoiceAction::Add;
        }
        if ui.button(texts::CANCEL).clicked() {
            action = ScanChoiceAction::Cancel;
        }
    });
    action
}

fn rows(ui: &mut Ui, offers: &[Offer]) -> ScanChoiceAction {
    let mut action = ScanChoiceAction::None;
    Grid::new(GRID_ID).striped(true).show(ui, |ui| {
        for (index, offer) in offers.iter().enumerate() {
            let mut checked = offer.checked;
            let checkbox = Checkbox::new(&mut checked, offer.name());
            if ui.add_enabled(!offer.known, checkbox).clicked() {
                action = ScanChoiceAction::Toggle(index);
            }
            ui.label(offer.model().unwrap_or_default());
            ui.label(offer.masked_address());
            if offer.known {
                ui.weak(texts::ALREADY_ADDED);
            }
            ui.end_row();
        }
    });
    action
}
