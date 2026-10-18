//! How a value the speaker reports reads on screen: the raw value with its unit, switches as
//! on/off, lists joined, and temperatures in whole degrees Celsius.

use neumann_control_core::catalog::{Conversion, Entry};
use serde_json::Value;

use crate::texts;

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
    let text = raw_text(value);
    match entry.unit.as_deref() {
        Some(unit) if !text.is_empty() => format!("{text} {unit}"),
        _ => text,
    }
}

/// The device reports whole kelvin, so tenths of a degree would only show rounding noise.
fn converted(conversion: Conversion, number: f64) -> String {
    let unit = match conversion {
        Conversion::KelvinToCelsius => texts::CELSIUS,
    };
    // Adding zero turns the -0 of a slightly negative reading into 0.
    let whole = conversion.apply(number).round() + 0.0;
    format!("{whole} {unit}")
}

fn raw_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(true) => texts::ON.to_owned(),
        Value::Bool(false) => texts::OFF.to_owned(),
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .map(raw_text)
            .collect::<Vec<_>>()
            .join(LIST_SEPARATOR),
        Value::Number(_) | Value::Object(_) => value.to_string(),
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
        assert_eq!(shown_value(enabled, &json!(true)), texts::ON);
        assert_eq!(shown_value(enabled, &json!(false)), texts::OFF);
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
