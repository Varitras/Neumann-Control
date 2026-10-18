//! How a value the speaker reports reads on screen: the raw value with its unit, switches as
//! on/off, lists joined, and temperatures in whole degrees Celsius.

use neumann_control_core::catalog::{Conversion, Entry};
use serde_json::Value;

use crate::texts;

/// Device text can be up to a line's 1 MiB; laid out whole it would stall every frame.
pub const VALUE_CHARACTER_LIMIT: usize = 200;
pub const ELLIPSIS: char = '…';
const LIST_SEPARATOR: &str = ", ";
const DEVICE_VERSION_SEPARATOR: char = '_';
const SHOWN_VERSION_SEPARATOR: char = '.';

/// The device reports its firmware as `major_minor_patch`; people write it with dots.
pub fn firmware_version(reported: &str) -> String {
    reported.replace(
        DEVICE_VERSION_SEPARATOR,
        &SHOWN_VERSION_SEPARATOR.to_string(),
    )
}

pub fn shown_value(entry: &Entry, value: &Value) -> String {
    if let (Some(conversion), Some(number)) = (entry.conversion, value.as_f64()) {
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

/// In the unit shown. The device reports whole kelvin, so tenths of a degree would only show
/// rounding noise; a delay sample is about 0.02 ms, so it needs two decimals.
pub fn converted(conversion: Conversion, number: f64) -> String {
    let shown = conversion.apply(number);
    let unit = converted_unit(conversion);
    match conversion {
        // Adding zero turns the -0 of a slightly negative reading into 0.
        Conversion::KelvinToCelsius => format!("{} {unit}", shown.round() + 0.0),
        Conversion::SamplesToMilliseconds => format!("{} {unit}", texts::two_decimals(shown)),
    }
}

/// The unit a converted value is shown in.
pub fn converted_unit(conversion: Conversion) -> &'static str {
    match conversion {
        Conversion::KelvinToCelsius => texts::CELSIUS,
        Conversion::SamplesToMilliseconds => texts::MILLISECONDS,
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
