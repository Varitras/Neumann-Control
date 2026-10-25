//! How a value the speaker reports reads on screen: the raw value with its unit, switches as
//! on/off, lists joined, and temperatures in whole degrees Celsius.

use std::cell::Cell;

use neumann_control_core::catalog::{Conversion, Entry};
use neumann_control_core::settings::DelayUnit;
use serde_json::Value;

use crate::texts;

/// Device text can be up to a line's 1 MiB; laid out whole it would stall every frame.
pub const VALUE_CHARACTER_LIMIT: usize = 200;
pub const ELLIPSIS: char = '…';
const LIST_SEPARATOR: &str = ", ";
const DEVICE_VERSION_SEPARATOR: char = '_';
const SHOWN_VERSION_SEPARATOR: char = '.';

thread_local! {
    /// Like the language: read on the UI thread only, per thread for parallel UI tests.
    static DELAY_UNIT: Cell<DelayUnit> = const { Cell::new(DelayUnit::Milliseconds) };
}

/// The unit every delay of this thread is shown and typed in from now on.
pub fn set_delay_unit(unit: DelayUnit) {
    DELAY_UNIT.with(|current| current.set(unit));
}

/// The conversion `entry`'s values are shown and typed with: the catalog's, with the delay in
/// the chosen unit. Every reader of a converted value goes through here.
pub fn conversion_of(entry: &Entry) -> Option<Conversion> {
    let meters = DELAY_UNIT.with(Cell::get) == DelayUnit::Meters;
    match entry.conversion {
        Some(Conversion::SamplesToMilliseconds) if meters => Some(Conversion::SamplesToMeters),
        conversion => conversion,
    }
}

/// The device reports its firmware as `major_minor_patch`; people write it with dots.
pub fn firmware_version(reported: &str) -> String {
    reported.replace(
        DEVICE_VERSION_SEPARATOR,
        &SHOWN_VERSION_SEPARATOR.to_string(),
    )
}

pub fn shown_value(entry: &Entry, value: &Value) -> String {
    if let (Some(conversion), Some(number)) = (conversion_of(entry), value.as_f64()) {
        return converted(conversion, number);
    }
    let text = capped(&raw_text(value));
    match entry.unit.as_deref() {
        Some(unit) if !text.is_empty() => format!("{text} {unit}"),
        _ => text,
    }
}

/// At most `VALUE_CHARACTER_LIMIT` characters, cut on a character boundary.
pub fn capped(text: &str) -> String {
    let mut characters = text.chars();
    let kept: String = characters.by_ref().take(VALUE_CHARACTER_LIMIT).collect();
    if characters.next().is_none() {
        return text.to_owned();
    }
    format!("{kept}{ELLIPSIS}")
}

/// In the unit shown.
pub fn converted(conversion: Conversion, number: f64) -> String {
    let unit = converted_unit(conversion);
    format!("{} {unit}", converted_number(conversion, number))
}

/// The number of `converted`, without its unit. The device reports whole kelvin, so tenths of a
/// degree would only show rounding noise; a delay sample is about 0.02 ms (7 mm), so it needs two
/// decimals.
pub fn converted_number(conversion: Conversion, number: f64) -> String {
    let shown = conversion.apply(number);
    match conversion {
        // Adding zero turns the -0 of a slightly negative reading into 0.
        Conversion::KelvinToCelsius => (shown.round() + 0.0).to_string(),
        Conversion::SamplesToMilliseconds | Conversion::SamplesToMeters => {
            texts::two_decimals(shown)
        }
    }
}

/// The unit a converted value is shown in.
pub fn converted_unit(conversion: Conversion) -> &'static str {
    match conversion {
        Conversion::KelvinToCelsius => texts::CELSIUS,
        Conversion::SamplesToMilliseconds => texts::MILLISECONDS,
        Conversion::SamplesToMeters => texts::METERS,
    }
}

fn raw_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(true) => texts::ON.to_string(),
        Value::Bool(false) => texts::OFF.to_string(),
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .map(raw_text)
            .collect::<Vec<_>>()
            .join(LIST_SEPARATOR),
        Value::Number(_) => texts::number_text(value.to_string()),
        Value::Object(_) => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neumann_control_core::catalog::Catalog;
    use serde_json::json;

    use crate::paths;

    fn entry(path: &str) -> &'static Entry {
        paths::catalog_entry(Catalog::embedded(), &paths::path(path)).unwrap()
    }

    #[test]
    fn a_long_device_text_is_shortened_on_a_character_boundary() {
        let long = "ä".repeat(VALUE_CHARACTER_LIMIT + 50);
        let shown = shown_value(entry("device/name"), &json!(long));
        assert_eq!(shown.chars().count(), VALUE_CHARACTER_LIMIT + 1);
        assert!(shown.ends_with(ELLIPSIS));
        let exact = "x".repeat(VALUE_CHARACTER_LIMIT);
        assert_eq!(capped(&exact), exact);
    }

    /// A delay field shows its value rounded and reads that text back when it is left: the
    /// text must land on the same sample, in milliseconds and in meters.
    #[test]
    fn every_shown_delay_reads_back_as_its_sample() {
        for conversion in [
            Conversion::SamplesToMilliseconds,
            Conversion::SamplesToMeters,
        ] {
            for samples in 0..=10_000 {
                let samples = f64::from(samples);
                let text = converted_number(conversion, samples);
                let typed = crate::gesture::typed_value(&text, Some(conversion)).unwrap();
                let read_back = crate::gesture::on_grid(typed, 1.0);
                assert_eq!(read_back, samples, "{conversion:?} {text}");
            }
        }
    }

    #[test]
    fn delay_is_shown_and_typed_in_meters_when_chosen() {
        let delay = entry("audio/out/delay");
        set_delay_unit(DelayUnit::Meters);
        let shown = shown_value(delay, &json!(480));
        let typed = crate::gesture::typed_value("3,43", conversion_of(delay));
        set_delay_unit(DelayUnit::Milliseconds);
        assert_eq!(shown, "3,43 m");
        assert!((typed.unwrap() - 480.0).abs() < 1e-9);
    }

    #[test]
    fn only_the_delay_follows_the_delay_unit() {
        set_delay_unit(DelayUnit::Meters);
        let temperature = conversion_of(entry("device/temperature"));
        set_delay_unit(DelayUnit::Milliseconds);
        assert_eq!(temperature, Some(Conversion::KelvinToCelsius));
    }

    #[test]
    fn delay_is_shown_in_milliseconds() {
        let delay = entry("audio/out/delay");
        assert_eq!(shown_value(delay, &json!(3732)), "77,75 ms");
        assert_eq!(shown_value(delay, &json!(0)), "0,00 ms");
    }

    #[test]
    fn firmware_underscores_become_dots() {
        assert_eq!(firmware_version("2_1_2"), "2.1.2");
    }

    #[test]
    fn firmware_without_underscores_is_unchanged() {
        assert_eq!(firmware_version("2.1.2"), "2.1.2");
        assert_eq!(firmware_version("1"), "1");
    }

    #[test]
    fn kelvin_is_shown_as_whole_degrees_celsius() {
        assert_eq!(
            shown_value(entry("device/temperature"), &json!(313)),
            "40 °C"
        );
        assert_eq!(
            shown_value(entry("device/temperature"), &json!(273)),
            "0 °C"
        );
    }

    #[test]
    fn a_rear_panel_string_keeps_its_unit() {
        assert_eq!(shown_value(entry("ui/bass_gain"), &json!("-2")), "-2 dB");
        assert_eq!(
            shown_value(entry("ui/input_select"), &json!("ANALOG")),
            "ANALOG"
        );
    }

    #[test]
    fn lists_are_joined_and_switches_read_as_on_or_off() {
        let warnings = json!(["NO_WARNING", "HOT"]);
        assert_eq!(shown_value(entry("warnings"), &warnings), "NO_WARNING, HOT");
        let enabled = entry("device/standby/enabled");
        assert_eq!(shown_value(enabled, &json!(true)), texts::ON.get());
        assert_eq!(shown_value(enabled, &json!(false)), texts::OFF.get());
    }

    #[test]
    fn a_missing_value_shows_no_unit() {
        assert_eq!(
            shown_value(entry("device/standby/countdown"), &Value::Null),
            ""
        );
        assert_eq!(
            shown_value(entry("device/standby/countdown"), &json!(5)),
            "5 min"
        );
    }
}
