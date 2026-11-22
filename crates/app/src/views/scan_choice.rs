//! The "Gefundene Lautsprecher" dialog after "Suchen": a spinner while the scan runs, then one
//! row per found speaker to check, or the usual causes when none answered. Speakers already shown
//! are listed but not offered. "Erneut suchen" scans again and keeps the rows and checks so far.

use eframe::egui::{Button, Checkbox, Grid, Id, Label, Modal, Ui};

use super::busy;
use crate::scan_choice::{Offer, ScanChoice};
use crate::texts::{self, Text};

const MODAL_ID: &str = "scan_choice";
const GRID_ID: &str = "scan_choice_rows";
const HINT_WIDTH: f32 = 360.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanChoiceAction {
    None,
    Toggle(usize),
    Add,
    Cancel,
    ScanAgain,
    OpenManual,
}

/// `scanning`: a scan runs, whose speakers will join the rows. `notice`: the app's notice, which
/// the toolbar shows behind the dialog's backdrop (the manual that could not be opened).
pub fn show(
    ui: &mut Ui,
    choice: &ScanChoice,
    scanning: bool,
    notice: Option<Text>,
) -> ScanChoiceAction {
    let modal = Modal::new(Id::unique(MODAL_ID))
        .show(ui.ctx(), |ui| contents(ui, choice, scanning, notice));
    if modal.should_close() {
        return ScanChoiceAction::Cancel;
    }
    modal.inner
}

fn contents(
    ui: &mut Ui,
    choice: &ScanChoice,
    scanning: bool,
    notice: Option<Text>,
) -> ScanChoiceAction {
    ui.heading(texts::SCAN_CHOICE_TITLE);
    let mut action = match choice {
        ScanChoice::Scanning => {
            busy::spinner(ui);
            ScanChoiceAction::None
        }
        ScanChoice::Failed => {
            ui.colored_label(ui.visuals().error_fg_color, texts::SCAN_FAILED);
            ScanChoiceAction::None
        }
        ScanChoice::Offered(offers) if offers.is_empty() => nothing_found(ui, notice),
        ScanChoice::Offered(offers) => rows(ui, offers),
    };
    ui.horizontal(|ui| {
        if ui
            .add_enabled(choice.can_add(), Button::new(texts::ADD))
            .clicked()
        {
            action = ScanChoiceAction::Add;
        }
        if ui
            .add_enabled(!scanning, Button::new(texts::SCAN_AGAIN))
            .clicked()
        {
            action = ScanChoiceAction::ScanAgain;
        }
        // The rows stay while a new scan runs; the spinner says that more may come.
        if scanning && !matches!(choice, ScanChoice::Scanning) {
            busy::spinner(ui);
        }
        if ui.button(texts::CANCEL).clicked() {
            action = ScanChoiceAction::Cancel;
        }
    });
    action
}

fn nothing_found(ui: &mut Ui, notice: Option<Text>) -> ScanChoiceAction {
    ui.label(texts::NOTHING_FOUND);
    // Without a limit the modal grows as wide as the sentence.
    ui.set_max_width(HINT_WIDTH);
    ui.add(Label::new(texts::SCAN_EMPTY_HINT).wrap());
    let opened = ui.button(texts::MANUAL).clicked();
    if let Some(notice) = notice {
        ui.add(Label::new(notice).wrap());
    }
    if opened {
        return ScanChoiceAction::OpenManual;
    }
    ScanChoiceAction::None
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
