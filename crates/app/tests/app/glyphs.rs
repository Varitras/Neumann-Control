//! Every UI text can be drawn with the bundled fonts.

use eframe::egui::{self, FontFamily, FontId};
use neumann_control_app::texts;
use neumann_control_app::ui_kit::fonts;
use neumann_control_core::address::AddressError;
use neumann_control_core::catalog::{Catalog, ValueError};
use neumann_control_core::mask::mask_serial;
use neumann_control_core::settings::Language;

const SIMULATED_SERIAL: &str = "SIM0001234";

/// Every string literal in texts.rs, so a new text is covered without being listed here.
fn literals_of_texts_module() -> Vec<String> {
    let source = include_str!("../../src/texts.rs");
    let mut literals = Vec::new();
    let mut rest = source;
    while let Some(start) = rest.find('"') {
        let after = &rest[start + 1..];
        let end = after.find('"').expect("closed literal");
        literals.push(after[..end].to_owned());
        rest = &after[end + 1..];
    }
    literals
}

#[test]
fn all_ui_texts_have_glyphs() {
    let context = egui::Context::default();
    fonts::install(&context);
    context
        .run_ui(egui::RawInput::default(), |_| {})
        .textures_delta
        .clear();
    let catalog = Catalog::embedded();
    let mut texts_to_check = literals_of_texts_module();
    for language in Language::ALL {
        texts::set_language(language);
        texts_to_check.extend(formatted_texts());
        for entry in &catalog.entries {
            texts_to_check.push(entry.group.title(language).to_owned());
            texts_to_check.push(entry.label(language).to_owned());
            texts_to_check.extend(entry.unit.clone());
        }
    }
    let families = [FontFamily::Proportional, fonts::semibold(12.0).family];
    for family in families {
        let font = FontId::new(12.0, family.clone());
        for text in &texts_to_check {
            let covered = context.fonts_mut(|fonts| fonts.has_glyphs(&font, text));
            assert!(covered, "{text:?} in {family:?}");
        }
    }
    assert!(texts_to_check
        .iter()
        .any(|text| text == texts::NO_SPEAKERS.get()));
}

/// The texts made by formatting, in the current language.
fn formatted_texts() -> Vec<String> {
    let mut texts_to_check = vec![
        texts::speaker_count(12),
        texts::port(45),
        texts::readout(-12.5, "dB SPL"),
        texts::status_line("1_7_3", &mask_serial(SIMULATED_SERIAL)),
        texts::serial_mismatch(&mask_serial(SIMULATED_SERIAL)),
        texts::describe_rejection(405),
        texts::describe_rejection(406),
        texts::describe_rejection(500),
    ];
    for error in [
        AddressError::Ipv4NotSupported,
        AddressError::MissingScope,
        AddressError::ScopeNotNumeric,
        AddressError::Invalid,
    ] {
        texts_to_check.push(texts::describe_address_error(&error));
    }
    for error in [
        ValueError::OutOfRange { min: 0.0, max: 1.0 },
        ValueError::NotAnOption,
        ValueError::NotAStep,
        ValueError::TooLong(52),
        ValueError::NotPrintableAscii,
        ValueError::WrongType,
    ] {
        texts_to_check.push(texts::describe_value_error(&error));
    }
    texts_to_check
}
