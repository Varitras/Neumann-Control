//! "Über": name, version and licence, the project page, and a check whether GitHub has a newer
//! release, which only links to it: the app does not update itself.

use eframe::egui::{Button, Id, Modal, RichText, Ui};

use crate::help::{UpdateResult, REPOSITORY_URL};
use crate::texts;

const MODAL_ID: &str = "about";

/// Where the update check stands while the dialog is open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateView {
    NotAsked,
    Checking,
    Done(UpdateResult),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AboutAction {
    None,
    Close,
    CheckForUpdates,
    /// A web page to open in the browser.
    Open(String),
}

pub fn show(ui: &mut Ui, update: &UpdateView) -> AboutAction {
    let modal = Modal::new(Id::unique(MODAL_ID)).show(ui.ctx(), |ui| contents(ui, update));
    if modal.should_close() {
        return AboutAction::Close;
    }
    modal.inner
}

fn contents(ui: &mut Ui, update: &UpdateView) -> AboutAction {
    let mut action = AboutAction::None;
    ui.heading(texts::WINDOW_TITLE);
    ui.label(texts::version(env!("CARGO_PKG_VERSION")));
    ui.label(texts::LICENCE);
    if ui.link(texts::PROJECT_PAGE).clicked() {
        action = AboutAction::Open(REPOSITORY_URL.to_owned());
    }
    ui.separator();
    let checking = *update == UpdateView::Checking;
    ui.horizontal(|ui| {
        let check = ui.add_enabled(!checking, Button::new(texts::CHECK_FOR_UPDATES));
        if check.clicked() {
            action = AboutAction::CheckForUpdates;
        }
        if checking {
            ui.spinner();
        }
    });
    if let UpdateView::Done(result) = update {
        if let Some(open) = outcome(ui, result) {
            action = open;
        }
    }
    ui.separator();
    if ui.button(texts::CLOSE).clicked() {
        action = AboutAction::Close;
    }
    action
}

fn outcome(ui: &mut Ui, result: &UpdateResult) -> Option<AboutAction> {
    let text = match result {
        UpdateResult::Current => texts::UP_TO_DATE.get(),
        UpdateResult::NoRelease => texts::NO_RELEASE.get(),
        UpdateResult::Unreachable => texts::UPDATE_CHECK_FAILED.get(),
        UpdateResult::Unusable => texts::UPDATE_CHECK_UNUSABLE.get(),
        UpdateResult::Newer(release) => {
            ui.label(RichText::new(texts::update_available(&release.tag)).strong());
            let open = ui.button(texts::OPEN_RELEASE_PAGE).clicked();
            return open.then(|| AboutAction::Open(release.url.clone()));
        }
    };
    ui.label(text);
    None
}
