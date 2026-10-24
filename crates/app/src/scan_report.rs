//! The report of "Komplett scannen": everything a speaker answered, with what the catalog knows
//! about each path, and nothing that identifies the speaker or its owner. No file access here.

use neumann_control_core::catalog::Catalog;
use neumann_control_core::schema_walk::LeafReport;
use serde_json::{json, Map, Value};

use crate::paths;

/// Stands for every value the report leaves out.
pub const REDACTED: &str = "<redacted>";
const MAC_GROUPS: usize = 6;
const MAC_GROUP_LENGTH: usize = 2;
/// A full IPv6 address has seven colons; a shortened one has `::`. A clock time has two at most.
const IPV6_FULL_COLONS: usize = 7;
const IPV6_SHORTENED: &str = "::";
const IPV4_GROUPS: usize = 4;
/// Punctuation around an address in a word; brackets are not among it, `[address]:port` is
/// told by them.
const WORD_TRIM: &str = ",;()\"'";

/// What the report says about the speaker, apart from its leaves.
pub struct ScanHeader<'a> {
    pub model: &'a str,
    pub firmware: &'a str,
    /// Local time of the scan, `yyyy-mm-dd hh:mm:ss`.
    pub time: &'a str,
}

/// `scan-<model>-<firmware>-<yyyymmdd-hhmmss>`, without the extension; never a serial or name.
pub fn file_stem(model: &str, firmware: &str, compact_time: &str) -> String {
    let safe = |text: &str| -> String {
        text.chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() || character == '_' {
                    character
                } else {
                    '-'
                }
            })
            .collect()
    };
    format!(
        "scan-{}-{}-{}",
        safe(model),
        safe(firmware),
        safe(compact_time)
    )
}

/// The leaves with every identifying value replaced: the listed paths, the network subtree, any
/// text that looks like a MAC or IPv6 address, and every occurrence of `known` (the speaker's
/// serial and name), wherever another leaf repeats them.
pub fn redacted(reports: &[LeafReport], known: &[&str]) -> Vec<LeafReport> {
    let known: Vec<&str> = known
        .iter()
        .copied()
        .filter(|text| !text.is_empty())
        .collect();
    reports
        .iter()
        .map(|report| {
            let path = report.path.to_string();
            let identifying = paths::is_identifier(&path) || path == paths::DEVICE_NAME;
            let value = match &report.value {
                _ if identifying => Ok(Value::String(REDACTED.to_owned())),
                Ok(value) => Ok(redacted_value(value, &known)),
                Err(text) => Err(redacted_text(text, &known)),
            };
            LeafReport {
                path: report.path.clone(),
                value,
                limits: report
                    .limits
                    .as_ref()
                    .map(|limits| redacted_value(limits, &known)),
            }
        })
        .collect()
}

fn redacted_value(value: &Value, known: &[&str]) -> Value {
    match value {
        Value::String(text) => Value::String(redacted_text(text, known)),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| redacted_value(item, known))
                .collect(),
        ),
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, value)| (key.clone(), redacted_value(value, known)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Replaces the known identifiers and each word that looks like an address; the rest stays.
fn redacted_text(text: &str, known: &[&str]) -> String {
    let text = known.iter().fold(text.to_owned(), |text, identifier| {
        text.replace(identifier, REDACTED)
    });
    let mut redacted = String::new();
    // Whitespace of any kind separates words; it stays as it was.
    for piece in text.split_inclusive(char::is_whitespace) {
        let word = piece.trim_end_matches(char::is_whitespace);
        let space = &piece[word.len()..];
        redacted.push_str(&redacted_word(word));
        redacted.push_str(space);
    }
    redacted
}

/// One word with the address in it replaced: `key=address`, `[address]:port` and
/// `address/prefix` are found as well as a bare address.
fn redacted_word(word: &str) -> String {
    let value = word.rsplit_once('=').map_or(word, |(_, value)| value);
    let bare = value.trim_matches(|character: char| WORD_TRIM.contains(character));
    let bracketed = bare
        .strip_prefix('[')
        .and_then(|rest| rest.split_once(']'))
        .map(|(inside, _)| inside);
    let address = bracketed.unwrap_or(bare);
    let address = address.split('/').next().unwrap_or_default();
    match !address.is_empty() && looks_like_address(address) {
        true => word.replace(address, REDACTED),
        false => word.to_owned(),
    }
}

fn looks_like_address(word: &str) -> bool {
    looks_like_mac(word) || looks_like_ipv6(word) || looks_like_ipv4(word)
}

fn looks_like_ipv4(word: &str) -> bool {
    let groups: Vec<&str> = word.split('.').collect();
    groups.len() == IPV4_GROUPS && groups.iter().all(|group| group.parse::<u8>().is_ok())
}

fn looks_like_mac(word: &str) -> bool {
    let groups: Vec<&str> = word.split([':', '-']).collect();
    groups.len() == MAC_GROUPS
        && groups.iter().all(|group| {
            group.len() == MAC_GROUP_LENGTH && group.chars().all(|digit| digit.is_ascii_hexdigit())
        })
}

fn looks_like_ipv6(word: &str) -> bool {
    let address = word.split('%').next().unwrap_or_default();
    let colons = address.matches(':').count();
    let allowed = address
        .chars()
        .all(|character| character.is_ascii_hexdigit() || character == ':' || character == '.');
    let has_digit = address
        .chars()
        .any(|character| character.is_ascii_hexdigit());
    let shaped = address.contains(IPV6_SHORTENED) || colons >= IPV6_FULL_COLONS;
    shaped && allowed && has_digit
}

/// The report as JSON: header, then per leaf the value or the refusal, the device's limits, and
/// what the catalog says for this model. `reports` must already be redacted.
pub fn json_report(header: &ScanHeader, reports: &[LeafReport], catalog: &Catalog) -> Value {
    let leaves: Vec<Value> = reports
        .iter()
        .map(|report| {
            let mut leaf = Map::new();
            leaf.insert("path".to_owned(), json!(report.path.to_string()));
            match &report.value {
                Ok(value) => leaf.insert("value".to_owned(), value.clone()),
                Err(text) => leaf.insert("error".to_owned(), json!(text)),
            };
            leaf.insert(
                "limits".to_owned(),
                report.limits.clone().unwrap_or(Value::Null),
            );
            let (known, writable_on) = catalog_view(catalog, header.model, report);
            leaf.insert("in_catalog".to_owned(), json!(known));
            leaf.insert("writable_on".to_owned(), json!(writable_on));
            Value::Object(leaf)
        })
        .collect();
    json!({
        "model": header.model,
        "firmware": header.firmware,
        "time": header.time,
        "leaves": leaves,
    })
}

/// The same as plain text, one line per leaf.
pub fn text_report(header: &ScanHeader, reports: &[LeafReport], catalog: &Catalog) -> String {
    let mut lines = vec![
        format!("model: {}", header.model),
        format!("firmware: {}", header.firmware),
        format!("time: {}", header.time),
        String::new(),
        "path | value or error | catalog | writable on | limits".to_owned(),
    ];
    for report in reports {
        let shown = match &report.value {
            Ok(value) => value.to_string(),
            Err(text) => format!("error: {text}"),
        };
        let (known, writable_on) = catalog_view(catalog, header.model, report);
        let catalog_column = if known { "known" } else { "not in catalog" };
        let limits = report
            .limits
            .as_ref()
            .map(Value::to_string)
            .unwrap_or_default();
        lines.push(format!(
            "{} | {shown} | {catalog_column} | {} | {limits}",
            report.path,
            writable_on.join(", ")
        ));
    }
    lines.join("\n") + "\n"
}

/// Whether the catalog lists the path for the model, and where it may be written.
fn catalog_view(catalog: &Catalog, model: &str, report: &LeafReport) -> (bool, Vec<String>) {
    let entry = catalog
        .entries_for(model)
        .into_iter()
        .find(|entry| entry.path == report.path);
    match entry {
        Some(entry) => (true, entry.writable_on.clone()),
        None => (false, Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neumann_control_core::ssc::wire::SscPath;

    fn leaf(path: &str, value: Value) -> LeafReport {
        LeafReport {
            path: SscPath::parse(path).unwrap(),
            value: Ok(value),
            limits: None,
        }
    }

    fn values(reports: &[LeafReport]) -> Vec<Value> {
        reports
            .iter()
            .map(|report| report.value.clone().unwrap_or_else(Value::String))
            .collect()
    }

    #[test]
    fn identifying_leaves_are_redacted() {
        let reports = [
            leaf("device/identity/serial", json!("3712345678")),
            leaf("device/network/ether/macs", json!("aa:bb:cc:dd:ee:ff")),
            leaf("device/name", json!("Living room left")),
            leaf("device/network/ipv6/address", json!(["x"])),
            leaf("device/identity/product", json!("KH 750")),
        ];
        let shown = values(&redacted(&reports, &[]));
        assert_eq!(shown[..4].to_vec(), vec![json!(REDACTED); 4]);
        assert_eq!(shown[4], json!("KH 750"));
    }

    #[test]
    fn address_like_text_is_redacted_anywhere() {
        let reports = [
            leaf("osc/some/text", json!("seen at fe80::1a2b:3c4d%7 now")),
            leaf("osc/list", json!(["aa-bb-cc-dd-ee-ff", "12:30"])),
            LeafReport {
                path: SscPath::parse("osc/refused").unwrap(),
                value: Err("from 2001:db8::1, refused".to_owned()),
                limits: Some(json!({"desc": "mac 00:11:22:33:44:55"})),
            },
        ];
        let shown = redacted(&reports, &[]);
        assert_eq!(shown[0].value, Ok(json!(format!("seen at {REDACTED} now"))));
        assert_eq!(shown[1].value, Ok(json!([REDACTED, "12:30"])));
        assert_eq!(shown[2].value, Err(format!("from {REDACTED}, refused")));
        assert_eq!(
            shown[2].limits,
            Some(json!({"desc": format!("mac {REDACTED}")}))
        );
    }

    #[test]
    fn addresses_with_prefix_port_key_or_tab_are_redacted() {
        let reports = [
            leaf("osc/a", json!("net fe80::1/64 up")),
            leaf("osc/b", json!("at [fe80::1]:45")),
            leaf("osc/c", json!("ip=fe80::1\tgw=192.168.1.1")),
            leaf("osc/d", json!("v4 192.168.1.20 seen")),
            leaf("osc/e", json!("mac=aa:bb:cc:dd:ee:ff")),
        ];
        let shown = values(&redacted(&reports, &[]));
        for value in &shown {
            let text = value.as_str().unwrap();
            for address in ["fe80::1", "192.168.1", "aa:bb:cc"] {
                assert!(!text.contains(address), "{text}");
            }
        }
    }

    /// A clock time has colons too, but no `::` and too few groups for IPv6.
    #[test]
    fn a_time_with_seconds_is_kept() {
        let shown = values(&redacted(&[leaf("osc/t", json!("at 19:04:05"))], &[]));
        assert_eq!(shown[0], json!("at 19:04:05"));
    }

    #[test]
    fn the_speakers_own_identifiers_are_redacted_wherever_they_appear() {
        let reports = [
            leaf("device/system", json!("unit 3712345678 ok")),
            leaf("osc/some/text", json!(["Living room left speaker"])),
        ];
        let shown = values(&redacted(&reports, &["3712345678", "Living room left"]));
        assert_eq!(shown[0], json!(format!("unit {REDACTED} ok")));
        assert_eq!(shown[1], json!([format!("{REDACTED} speaker")]));
    }

    #[test]
    fn the_file_name_holds_model_firmware_and_time_only() {
        assert_eq!(
            file_stem("KH 750", "2_1_2", "20261020-190405"),
            "scan-KH-750-2_1_2-20261020-190405"
        );
    }

    #[test]
    fn the_report_says_what_the_catalog_knows() {
        let catalog = Catalog::embedded();
        let header = ScanHeader {
            model: "KH 120 II",
            firmware: "1_7_3",
            time: "2026-10-20 19:04:05",
        };
        let reports = [
            leaf("audio/out/level", json!(100.0)),
            leaf("device/update/enable", json!(false)),
        ];
        let report = json_report(&header, &reports, catalog);
        let leaves = report["leaves"].as_array().unwrap();
        assert_eq!(leaves[0]["in_catalog"], json!(true));
        assert_eq!(leaves[0]["writable_on"], json!(["KH 120 II", "KH 750"]));
        assert_eq!(leaves[1]["in_catalog"], json!(false));
        let text = text_report(&header, &reports, catalog);
        assert!(text.contains("audio/out/level | 100.0 | known | KH 120 II, KH 750 |"));
        assert!(text.contains("device/update/enable | false | not in catalog |"));
    }
}
