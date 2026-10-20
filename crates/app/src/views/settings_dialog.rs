//! The settings dialog behind the toolbar's "Einstellungen": theme, language, meter range and
//! response times. Every change applies at once; the app saves it.

use eframe::egui::ComboBox;
use eframe::egui::{Id, Modal, Ui};
use neumann_control_core::settings::{
    CrossoverGuard, DeskSettings, LanguageChoice, MeterFloor, SystemAudioSettings, ThemeChoice,
};

use crate::system_audio::AudioDevice;

use crate::texts;

const MODAL_ID: &str = "settings_dialog";
const SYSTEM_DEVICE_ID: &str = "system_audio_device";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsAction {
    None,
    Changed,
    Close,
}

pub fn show(
    ui: &mut Ui,
    desk: &mut DeskSettings,
    audio: &mut SystemAudioSettings,
    devices: &[AudioDevice],
) -> SettingsAction {
    let modal =
        Modal::new(Id::new(MODAL_ID)).show(ui.ctx(), |ui| contents(ui, desk, audio, devices));
    if modal.should_close() {
        return SettingsAction::Close;
    }
    modal.inner
}

fn contents(
    ui: &mut Ui,
    desk: &mut DeskSettings,
    audio: &mut SystemAudioSettings,
    devices: &[AudioDevice],
) -> SettingsAction {
    let before = *desk;
    let audio_before = audio.clone();
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
    system_audio(ui, audio, devices);
    ui.separator();
    if ui.button(texts::CLOSE).clicked() {
        return SettingsAction::Close;
    }
    if *desk != before || *audio != audio_before {
        return SettingsAction::Changed;
    }
    SettingsAction::None
}

/// Whether the desk shows the Windows volume, and of which playback device.
fn system_audio(ui: &mut Ui, audio: &mut SystemAudioSettings, devices: &[AudioDevice]) {
    ui.checkbox(&mut audio.shown, texts::SYSTEM_AUDIO_SHOW);
    if !audio.shown {
        return;
    }
    let chosen = match &audio.device {
        None => texts::SYSTEM_AUDIO_DEFAULT.get().to_owned(),
        Some(id) => devices.iter().find(|device| device.id == *id).map_or_else(
            || texts::SYSTEM_AUDIO_UNAVAILABLE.get().to_owned(),
            |device| device.name.clone(),
        ),
    };
    ui.horizontal(|ui| {
        let label = ui.label(texts::SYSTEM_AUDIO_DEVICE);
        ComboBox::from_id_salt(SYSTEM_DEVICE_ID)
            .selected_text(chosen)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut audio.device, None, texts::SYSTEM_AUDIO_DEFAULT);
                for device in devices {
                    let id = Some(device.id.clone());
                    ui.selectable_value(&mut audio.device, id, &device.name);
                }
            })
            .response
            .labelled_by(label.id);
    });
}
