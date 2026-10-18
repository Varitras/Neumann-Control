//! What the "Alle Werte" table shows: one row per leaf of the device schema, filtered, with
//! identifiers masked and device text capped. No drawing here.

use neumann_control_core::catalog::Catalog;
use neumann_control_core::mask::mask_serial;
use neumann_control_core::schema_walk::LeafReport;
use serde_json::Value;

use crate::paths;

/// A device can answer with any amount of text; one table cell must not grow with it.
pub const VALUE_CHARACTER_LIMIT: usize = 200;
const ELLIPSIS: char = '…';
const COPY_SEPARATOR: &str = " = ";
const RANGE_SEPARATOR: &str = "  ";
const OPTION_SEPARATOR: &str = " | ";
const BOUNDS_SEPARATOR: &str = " … ";
const UNITS_KEY: &str = "units";
const MINIMUM_KEY: &str = "min";
const MAXIMUM_KEY: &str = "max";
const OPTIONS_KEY: &str = "option";
/// Leaves that name the device itself; shown masked like the serial on the card.
const IDENTIFIER_PATHS: &[&str] = &[paths::SERIAL, paths::MAC_ADDRESSES];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub path: String,
    /// Capped for the cell.
    pub value: String,
    /// Units, range and options from the leaf's limits.
    pub range: String,
    pub known: bool,
    /// `path = value`, whole.
    pub copy_text: String,
}

/// The reports whose path contains `filter`, ignoring case and surrounding blanks.
pub fn rows(reports: &[LeafReport], filter: &str, catalog: &Catalog) -> Vec<Row> {
    let wanted = filter.trim().to_lowercase();
    reports
        .iter()
        .filter(|report| report.path.to_string().to_lowercase().contains(&wanted))
        .map(|report| row(report, catalog))
        .collect()
}

fn row(report: &LeafReport, catalog: &Catalog) -> Row {
    let path = report.path.to_string();
    let full = value_text(report, &path);
    Row {
        value: capped(&full),
        range: capped(&range_text(report.limits.as_ref())),
        known: paths::catalog_entry(catalog, &report.path).is_some(),
        copy_text: format!("{path}{COPY_SEPARATOR}{full}"),
        path,
    }
}

fn value_text(report: &LeafReport, path: &str) -> String {
    let text = match &report.value {
        Ok(value) => plain_text(value),
        Err(refusal) => refusal.clone(),
    };
    if IDENTIFIER_PATHS.contains(&path) {
        return mask_serial(&text);
    }
    text
}

/// Strings without their quotes, everything else as JSON.
fn plain_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn range_text(limits: Option<&Value>) -> String {
    let Some(limits) = limits else {
        return String::new();
    };
    let mut parts = Vec::new();
    if let Some(units) = limits.get(UNITS_KEY).map(plain_text) {
        parts.push(units);
    }
    let bounds = (limits.get(MINIMUM_KEY), limits.get(MAXIMUM_KEY));
    if bounds != (None, None) {
        let [minimum, maximum] = [bounds.0, bounds.1].map(|bound| bound.map(plain_text));
        parts.push(format!(
            "{}{BOUNDS_SEPARATOR}{}",
            minimum.unwrap_or_default(),
            maximum.unwrap_or_default()
        ));
    }
    if let Some(options) = limits.get(OPTIONS_KEY) {
        parts.push(options_text(options));
    }
    parts.join(RANGE_SEPARATOR)
}

fn options_text(options: &Value) -> String {
    match options {
        Value::Array(items) => items
            .iter()
            .map(plain_text)
            .collect::<Vec<_>>()
            .join(OPTION_SEPARATOR),
        other => plain_text(other),
    }
}

/// At most `VALUE_CHARACTER_LIMIT` characters, cut on a character boundary.
fn capped(text: &str) -> String {
    let mut characters = text.chars();
    let kept: String = characters.by_ref().take(VALUE_CHARACTER_LIMIT).collect();
    if characters.next().is_none() {
        return text.to_owned();
    }
    format!("{kept}{ELLIPSIS}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use neumann_control_core::ssc::wire::SscPath;
    use serde_json::json;

    fn report(path: &str, value: Result<Value, String>, limits: Option<Value>) -> LeafReport {
        LeafReport {
            path: SscPath::parse(path).unwrap(),
            value,
            limits,
        }
    }

    fn one(report: LeafReport) -> Row {
        rows(&[report], "", Catalog::embedded()).remove(0)
    }

    #[test]
    fn filter_ignores_case_and_blanks() {
        let reports = [
            report("device/standby/level", Ok(json!(-65)), None),
            report("audio/out/level", Ok(json!(100.0)), None),
        ];
        let found = rows(&reports, "  STANDBY ", Catalog::embedded());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, "device/standby/level");
        assert_eq!(rows(&reports, "", Catalog::embedded()).len(), 2);
    }

    #[test]
    fn long_text_is_capped_in_the_cell_but_copied_whole() {
        let long = "ä".repeat(VALUE_CHARACTER_LIMIT + 50);
        let row = one(report("device/name", Ok(json!(long)), None));
        assert_eq!(row.value.chars().count(), VALUE_CHARACTER_LIMIT + 1);
        assert!(row.value.ends_with(ELLIPSIS));
        assert_eq!(row.copy_text, format!("device/name = {long}"));
    }

    #[test]
    fn text_at_the_limit_is_not_cut() {
        let exact = "x".repeat(VALUE_CHARACTER_LIMIT);
        let row = one(report("device/name", Ok(json!(exact)), None));
        assert_eq!(row.value, exact);
    }

    #[test]
    fn identifiers_are_masked_in_cell_and_copy() {
        for path in IDENTIFIER_PATHS {
            let row = one(report(path, Ok(json!("SIM0001234")), None));
            assert_eq!(row.value, "…234", "{path}");
            assert_eq!(row.copy_text, format!("{path} = …234"));
        }
    }

    #[test]
    fn a_refusal_is_the_value_text() {
        let row = one(report("warnings", Err("refused (404)".to_owned()), None));
        assert_eq!(row.value, "refused (404)");
    }

    #[test]
    fn known_means_in_the_catalog() {
        assert!(one(report(paths::OUTPUT_LEVEL, Ok(json!(1)), None)).known);
        assert!(!one(report("device/restore", Ok(json!("")), None)).known);
    }

    #[test]
    fn range_joins_units_bounds_and_options() {
        let limits = json!({ "type": "number", "units": "dB", "min": 0, "max": 120 });
        let row = one(report("a/b", Ok(json!(1)), Some(limits)));
        assert_eq!(row.range, "dB  0 … 120");
        let options = json!({ "option": ["ON", "OFF"] });
        let row = one(report("a/b", Ok(json!("ON")), Some(options)));
        assert_eq!(row.range, "ON | OFF");
        let row = one(report(
            "a/b",
            Ok(json!(1)),
            Some(json!({ "type": "number" })),
        ));
        assert_eq!(row.range, "");
        assert_eq!(one(report("a/b", Ok(json!(1)), None)).range, "");
    }
}
