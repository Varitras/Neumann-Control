//! What the "Alle Werte" table shows: one row per leaf of the device schema, filtered, with
//! identifiers masked and device text capped. No drawing here.

use neumann_control_core::catalog::Catalog;
use neumann_control_core::mask::mask_serial;
use neumann_control_core::schema_walk::LeafReport;
use serde_json::Value;

use crate::display::capped;
use crate::paths;

/// A device can answer with any amount of text; one table cell must not grow with it.
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

#[derive(Clone, Debug, PartialEq)]
pub struct Row<'a> {
    report: &'a LeafReport,
    pub path: String,
    /// Capped for the cell.
    pub value: String,
    /// Units, range and options from the leaf's limits.
    pub range: String,
    pub known: bool,
}

impl Row<'_> {
    /// `path = value`, whole; built on demand because only a click needs it.
    pub fn copy_text(&self) -> String {
        let value = value_text(self.report, &self.path);
        format!("{}{COPY_SEPARATOR}{value}", self.path)
    }
}

/// The reports whose path contains `filter`, ignoring case and surrounding blanks.
pub fn rows<'a>(reports: &'a [LeafReport], filter: &str, catalog: &Catalog) -> Vec<Row<'a>> {
    let wanted = filter.trim().to_lowercase();
    reports
        .iter()
        .filter(|report| report.path.to_string().to_lowercase().contains(&wanted))
        .map(|report| row(report, catalog))
        .collect()
}

fn row<'a>(report: &'a LeafReport, catalog: &Catalog) -> Row<'a> {
    let path = report.path.to_string();
    Row {
        report,
        value: capped(&value_text(report, &path)),
        range: capped(&range_text(report.limits.as_ref())),
        known: paths::catalog_entry(catalog, &report.path).is_some(),
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
    let given = |key: &str| limits.get(key).filter(|value| !value.is_null());
    let mut parts = Vec::new();
    if let Some(units) = given(UNITS_KEY).map(plain_text) {
        parts.push(units);
    }
    let bounds = (given(MINIMUM_KEY), given(MAXIMUM_KEY));
    if bounds != (None, None) {
        let [minimum, maximum] = [bounds.0, bounds.1].map(|bound| bound.map(plain_text));
        parts.push(format!(
            "{}{BOUNDS_SEPARATOR}{}",
            minimum.unwrap_or_default(),
            maximum.unwrap_or_default()
        ));
    }
    if let Some(options) = given(OPTIONS_KEY) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::{ELLIPSIS, VALUE_CHARACTER_LIMIT};
    use neumann_control_core::ssc::wire::SscPath;
    use serde_json::json;

    fn report(path: &str, value: Result<Value, String>, limits: Option<Value>) -> LeafReport {
        LeafReport {
            path: SscPath::parse(path).unwrap(),
            value,
            limits,
        }
    }

    /// The report is leaked so the row may outlive the call; tests only.
    fn one(report: LeafReport) -> Row<'static> {
        let leaked: &'static [LeafReport] = Box::leak(Box::new([report]));
        rows(leaked, "", Catalog::embedded()).remove(0)
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
        assert_eq!(row.copy_text(), format!("device/name = {long}"));
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
            assert_eq!(row.copy_text(), format!("{path} = …234"));
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
    fn null_limits_are_absent() {
        let limits = json!({ "units": null, "min": null, "max": 120, "option": null });
        let row = one(report("a/b", Ok(json!(1)), Some(limits)));
        assert_eq!(row.range, " … 120");
        let all_null = json!({ "units": null, "min": null, "max": null, "option": null });
        assert_eq!(one(report("a/b", Ok(json!(1)), Some(all_null))).range, "");
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
