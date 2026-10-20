//! The settings dialog behind the toolbar's "Einstellungen": theme, language, meter range and
//! response times. Every change applies at once; the app saves it.

use eframe::egui::{Id, Modal, Ui};
use neumann_control_core::settings::{
    CrossoverGuard, DeskSettings, LanguageChoice, MeterFloor, ThemeChoice,
};

use crate::texts;

const MODAL_ID: &str = "settings_dialog";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsAction {
    None,
    Changed,
    Close,
}

pub fn show(ui: &mut Ui, desk: &mut DeskSettings) -> SettingsAction {
    let modal = Modal::new(Id::new(MODAL_ID)).show(ui.ctx(), |ui| contents(ui, desk));
    if modal.should_close() {
        return SettingsAction::Close;
    }
    modal.inner
}

fn contents(ui: &mut Ui, desk: &mut DeskSettings) -> SettingsAction {
    let before = *desk;
    ui.heading(texts::SETTINGS_TITLE);
    ui.label(texts::THEME);
    ui.horizontal(|ui| {
        ui.radio_value(&mut desk.theme, ThemeChoice::System, texts::LIKE_WINDOWS);
        ui.radio_value(&mut desk.theme, ThemeChoice::Light, texts::THEME_LIGHT);
        ui.radio_value(&mut desk.theme, ThemeChoice::Dark, texts::THEME_DARK);
    });
    ui.label(texts::LANGUAGE_LABEL);
    ui.horizontal(|ui| {
        let language = &mut desk.language;
        ui.radio_value(language, LanguageChoice::System, texts::LIKE_WINDOWS);
        ui.radio_value(language, LanguageChoice::German, texts::LANGUAGE_GERMAN);
        ui.radio_value(language, LanguageChoice::English, texts::LANGUAGE_ENGLISH);
    });
    ui.label(texts::METER_RANGE);
    ui.horizontal(|ui| {
        for floor in MeterFloor::ALL {
            ui.radio_value(&mut desk.meter_floor, floor, texts::meter_floor(floor.db()));
        }
    });
    ui.checkbox(&mut desk.show_response_times, texts::SHOW_RESPONSE_TIMES);
    let mut protected = desk.crossovers == CrossoverGuard::Protected;
    if ui
        .checkbox(&mut protected, texts::PROTECT_CROSSOVERS)
        .changed()
    {
        desk.crossovers = if protected {
            CrossoverGuard::Protected
        } else {
            CrossoverGuard::Open
        };
    }
    ui.separator();
    if ui.button(texts::CLOSE).clicked() {
        return SettingsAction::Close;
    }
    if *desk != before {
        return SettingsAction::Changed;
    }
    SettingsAction::None
}
