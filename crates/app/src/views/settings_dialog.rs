//! The settings behind the toolbar's "Einstellungen", in a window of their own: how the desk
//! looks and behaves, the Windows strip and the EQ graph's display. Every change applies at
//! once; the app saves it. Escape, **Close** or the window's close button close it.

use eframe::egui::color_picker::{color_edit_button_srgba, Alpha};
use eframe::egui::{
    self, ComboBox, Context, Grid, Id, ScrollArea, Theme, Ui, Vec2, ViewportBuilder, ViewportClass,
    ViewportId,
};
use neumann_control_core::settings::{
    BandCurves, CrossoverGuard, DelayUnit, EqDisplaySettings, LanguageChoice, MeterFloor, Settings,
    SystemAudioSettings, ThemeChoice,
};

use super::{escape_closes, placed, Opening};
use crate::system_audio::AudioDevice;
use crate::texts;
use crate::ui_kit::eq_palette::{band_color, band_key, curve_color, hex, BAND_COLOR_COUNT};

const WINDOW_ID: &str = "settings_window";
const SETTINGS_WINDOW_SIZE: [f32; 2] = [520.0, 640.0];
const SETTINGS_WINDOW_MIN_SIZE: [f32; 2] = [360.0, 300.0];
const SYSTEM_DEVICE_ID: &str = "system_audio_device";
const METER_FLOOR_ID: &str = "meter_floor";
const BAND_GRID_ID: &str = "band_colors";
const BAND_COLUMNS: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsAction {
    None,
    Changed,
    Close,
}

/// The settings window's viewport, to read where the user left it.
pub fn viewport() -> ViewportId {
    ViewportId::from_hash_of(Id::unique(WINDOW_ID))
}

/// `curve_labels`: each curve's settings key with its name, as the EQ window's tabs say it.
pub fn show(
    context: &Context,
    opening: Opening,
    settings: &mut Settings,
    devices: &[AudioDevice],
    curve_labels: &[(String, String)],
) -> SettingsAction {
    let builder = ViewportBuilder::default()
        .with_title(texts::SETTINGS_TITLE.get())
        .with_min_inner_size(SETTINGS_WINDOW_MIN_SIZE);
    let builder = placed(builder, opening, Vec2::from(SETTINGS_WINDOW_SIZE));
    context.show_viewport_immediate(viewport(), builder, |ui, class| {
        super::note_repaint_causes(ui);
        let mut scrolled = |ui: &mut Ui| {
            ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| contents(ui, settings, devices, curve_labels))
                .inner
        };
        let action = match class {
            ViewportClass::EmbeddedWindow => scrolled(ui),
            _ => egui::CentralPanel::default().show(ui, scrolled).inner,
        };
        let escape = escape_closes(ui, Id::unique(WINDOW_ID));
        let close_requested = ui.input(|input| input.viewport().close_requested());
        if escape || close_requested {
            return SettingsAction::Close;
        }
        action
    })
}

fn contents(
    ui: &mut Ui,
    settings: &mut Settings,
    devices: &[AudioDevice],
    curve_labels: &[(String, String)],
) -> SettingsAction {
    let before = (
        settings.desk.clone(),
        settings.system_audio.clone(),
        settings.eq_display.clone(),
    );
    let desk = &mut settings.desk;
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
    ui.horizontal(|ui| {
        let label = ui.label(texts::METER_RANGE);
        ComboBox::from_id_salt(METER_FLOOR_ID)
            .selected_text(texts::meter_floor(desk.meter_floor.db()))
            .show_ui(ui, |ui| {
                for floor in MeterFloor::ALL {
                    let text = texts::meter_floor(floor.db());
                    ui.selectable_value(&mut desk.meter_floor, floor, text);
                }
            })
            .response
            .labelled_by(label.id);
    });
    ui.label(texts::DELAY_SHOWN_IN);
    ui.horizontal(|ui| {
        let unit = &mut desk.delay_unit;
        ui.radio_value(unit, DelayUnit::Milliseconds, texts::DELAY_IN_MILLISECONDS);
        ui.radio_value(unit, DelayUnit::Meters, texts::DELAY_IN_METERS);
    });
    ui.checkbox(&mut desk.show_response_times, texts::SHOW_RESPONSE_TIMES);
    ui.checkbox(&mut desk.show_master, texts::SHOW_MASTER);
    let mut per_model = !desk.layouts_per_speaker;
    if ui
        .checkbox(&mut per_model, texts::LAYOUTS_PER_MODEL)
        .changed()
    {
        desk.layouts_per_speaker = !per_model;
    }
    ui.checkbox(&mut desk.global_shortcuts, texts::GLOBAL_SHORTCUTS);
    ui.checkbox(&mut desk.detailed_log, texts::DETAILED_LOG)
        .on_hover_text(texts::DETAILED_LOG_HINT);
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
        settings.desk.clone(),
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
                ui.label(band_key(band));
                if color_edit_button_srgba(ui, &mut color, Alpha::Opaque).changed() {
                    display.band_colors.insert(band_key(band), hex(color));
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
