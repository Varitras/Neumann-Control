//! The settings dialog behind the toolbar's "Einstellungen": theme, language, meter range,
//! response times, crossovers, the Windows strip and the EQ graph's display. Every change applies
//! at once; the app saves it.

use eframe::egui::color_picker::{color_edit_button_srgba, Alpha};
use eframe::egui::{ComboBox, Grid, Id, Modal, ScrollArea, Theme, Ui};
use neumann_control_core::settings::{
    BandCurves, CrossoverGuard, EqDisplaySettings, LanguageChoice, MeterFloor, Settings,
    SystemAudioSettings, ThemeChoice,
};

use crate::system_audio::AudioDevice;
use crate::texts;
use crate::ui_kit::eq_palette::{band_color, curve_color, hex, BAND_COLOR_COUNT};

const MODAL_ID: &str = "settings_dialog";
const SYSTEM_DEVICE_ID: &str = "system_audio_device";
const BAND_GRID_ID: &str = "band_colors";
const BAND_COLUMNS: usize = 10;
/// Taller than this, the dialog scrolls.
const DIALOG_MAX_HEIGHT: f32 = 600.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsAction {
    None,
    Changed,
    Close,
}

/// `curve_labels`: each curve's settings key with its name, as the EQ window's tabs say it.
pub fn show(
    ui: &mut Ui,
    settings: &mut Settings,
    devices: &[AudioDevice],
    curve_labels: &[(String, String)],
) -> SettingsAction {
    let modal = Modal::new(Id::unique(MODAL_ID)).show(ui.ctx(), |ui| {
        ScrollArea::vertical()
            .max_height(DIALOG_MAX_HEIGHT)
            .show(ui, |ui| contents(ui, settings, devices, curve_labels))
            .inner
    });
    if modal.should_close() {
        return SettingsAction::Close;
    }
    modal.inner
}

fn contents(
    ui: &mut Ui,
    settings: &mut Settings,
    devices: &[AudioDevice],
    curve_labels: &[(String, String)],
) -> SettingsAction {
    let before = (
        settings.desk,
        settings.system_audio.clone(),
        settings.eq_display.clone(),
    );
    let desk = &mut settings.desk;
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
    system_audio(ui, &mut settings.system_audio, devices);
    ui.separator();
    eq_display(ui, &mut settings.eq_display, curve_labels);
    ui.separator();
    if ui.button(texts::CLOSE).clicked() {
        return SettingsAction::Close;
    }
    let after = (
        settings.desk,
        settings.system_audio.clone(),
        settings.eq_display.clone(),
    );
    if after != before {
        return SettingsAction::Changed;
    }
    SettingsAction::None
}

/// Which bands show their own curve, and the colours of bands and curves.
fn eq_display(ui: &mut Ui, display: &mut EqDisplaySettings, curve_labels: &[(String, String)]) {
    let dark = ui.ctx().theme() == Theme::Dark;
    ui.strong(texts::EQ_DISPLAY);
    ui.radio_value(
        &mut display.band_curves,
        BandCurves::All,
        texts::BAND_CURVES_ALL,
    );
    ui.radio_value(
        &mut display.band_curves,
        BandCurves::ChosenAndHovered,
        texts::BAND_CURVES_CHOSEN,
    );
    ui.label(texts::BAND_COLORS);
    Grid::new(BAND_GRID_ID).show(ui, |ui| {
        for band in 0..BAND_COLOR_COUNT {
            let mut color = band_color(display, band, dark);
            ui.horizontal(|ui| {
                ui.label((band + 1).to_string());
                if color_edit_button_srgba(ui, &mut color, Alpha::Opaque).changed() {
                    display
                        .band_colors
                        .insert((band + 1).to_string(), hex(color));
                }
            });
            if (band + 1) % BAND_COLUMNS == 0 {
                ui.end_row();
            }
        }
    });
    ui.label(texts::CURVE_COLORS);
    ui.horizontal_wrapped(|ui| {
        for (key, label) in curve_labels {
            let mut color = curve_color(display, key);
            ui.label(label);
            if color_edit_button_srgba(ui, &mut color, Alpha::Opaque).changed() {
                display.curve_colors.insert(key.clone(), hex(color));
            }
        }
    });
    if ui.button(texts::DEFAULT_COLORS).clicked() {
        display.band_colors.clear();
        display.curve_colors.clear();
    }
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
