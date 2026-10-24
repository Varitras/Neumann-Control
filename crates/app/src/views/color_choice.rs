//! The colour of a strip, chosen in the group dialog and the device panel: its default, none,
//! or one of the user's own.

use eframe::egui::color_picker::{color_edit_button_srgba, Alpha};
use eframe::egui::{Color32, Ui};
use neumann_control_core::settings::ColorChoice;

use crate::texts;
use crate::ui_kit::eq_palette::{hex, parse_hex};

/// `default_label` names what the default is ("Standard", "Wie die Gruppe"); `start` is the
/// colour an own choice starts from. True when the user changed `choice`.
pub(super) fn edit(
    ui: &mut Ui,
    default_label: &str,
    choice: &mut ColorChoice,
    start: Color32,
) -> bool {
    let before = choice.clone();
    ui.horizontal(|ui| {
        ui.label(texts::STRIP_COLOR);
        if ui.radio(choice.is_default(), default_label).clicked() {
            *choice = ColorChoice::Default;
        }
        if ui
            .radio(*choice == ColorChoice::Off, texts::COLOR_OFF)
            .clicked()
        {
            *choice = ColorChoice::Off;
        }
        let own = matches!(choice, ColorChoice::Own(_));
        if ui.radio(own, texts::COLOR_OWN).clicked() && !own {
            *choice = ColorChoice::Own(hex(start));
        }
        if let ColorChoice::Own(text) = choice {
            let mut color = parse_hex(text).unwrap_or(start);
            if color_edit_button_srgba(ui, &mut color, Alpha::Opaque).changed() {
                *text = hex(color);
            }
        }
    });
    *choice != before
}
