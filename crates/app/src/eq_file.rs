//! EQ filters as text files: the filter lines of Equalizer APO's `config.txt`, which are also
//! what Room EQ Wizard exports for its "Generic" equaliser (`Filter 1: ON PK Fc 100 Hz Gain -3.0
//! dB Q 1.41`). Import maps them onto a container's bands; export writes a container as APO lines.
//! Shelves are left out both ways until it is measured how the speakers define their Q.

use std::collections::BTreeMap;

use neumann_control_core::settings::EqPreset;
use serde_json::Value;

use crate::eq::{Band, Leaf};

/// Bigger than any filter file; a larger file is not one.
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;

const COMMENT: char = '#';
const FILTER_COMMAND: &str = "filter";
const BYTE_ORDER_MARK: char = '\u{feff}';
/// APO commands that change the sound but have no place in one EQ container; anything else
/// before a colon (REW's header lines `Dated:`, `Notes:` ...) is passed over silently.
const SOUND_COMMANDS: [&str; 14] = [
    "preamp",
    "channel",
    "include",
    "graphiceq",
    "device",
    "delay",
    "copy",
    "convolution",
    "stage",
    "eval",
    "if",
    "elseif",
    "else",
    "loudnesscorrection",
];

/// A filter as the speaker takes it: `boost` is the filter's height, `gain` stays 0.
#[derive(Clone, Debug, PartialEq)]
pub struct Filter {
    pub kind: &'static str,
    pub frequency: f64,
    pub boost: f64,
    pub q: f64,
}

/// What a file holds: per band (zero-based) a filter, or `None` for a band the file switches
/// off; and the sound commands that were not taken over.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Imported {
    pub bands: BTreeMap<usize, Option<Filter>>,
    pub ignored: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ImportError {
    /// `line` is one-based.
    Line {
        line: usize,
        problem: LineProblem,
    },
    NoFilters,
    /// The file names a band the container does not have (one-based).
    TooManyBands {
        band: usize,
        bands: usize,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum LineProblem {
    UnknownType(String),
    ShelfNotSupported(String),
    Missing(Parameter),
    BadNumber(String),
    UnknownState(String),
    /// A second line for the same band (one-based).
    SameBand(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parameter {
    Frequency,
    Gain,
    Q,
}

/// Which parameters a file type needs, and the speaker's type for it.
struct FileType {
    kind: &'static str,
    gain: bool,
}

fn file_type(token: &str) -> Result<Option<FileType>, LineProblem> {
    let upper = token.to_ascii_uppercase();
    let shaped = |kind, gain| Ok(Some(FileType { kind, gain }));
    match upper.as_str() {
        "PK" | "PEQ" | "MODAL" => shaped("PARAMETRIC", true),
        "LP" | "LPQ" => shaped("LOWPASS", false),
        "HP" | "HPQ" => shaped("HIGHPASS", false),
        "BP" => shaped("BANDPASS", false),
        "NO" => shaped("NOTCH", false),
        "AP" => shaped("ALLPASS", false),
        "NONE" => Ok(None),
        "LS" | "HS" | "LSC" | "HSC" => Err(LineProblem::ShelfNotSupported(token.to_owned())),
        _ => Err(LineProblem::UnknownType(token.to_owned())),
    }
}

/// The filters of an APO or REW text. Filter lines with a number go to that band, lines
/// without one to the band after the previous line.
pub fn parse(text: &str) -> Result<Imported, ImportError> {
    let mut imported = Imported::default();
    let mut next_band = 0;
    let text = text.trim_start_matches(BYTE_ORDER_MARK);
    for (place, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.starts_with(COMMENT) {
            continue;
        }
        let Some((command, rest)) = line.split_once(':') else {
            continue;
        };
        let command = command.trim();
        let lower = command.to_ascii_lowercase();
        let failed = |problem| ImportError::Line {
            line: place + 1,
            problem,
        };
        let Some(number) = lower.strip_prefix(FILTER_COMMAND) else {
            note_ignored(&mut imported, command, &lower);
            continue;
        };
        let band = match number.trim() {
            "" => next_band,
            given => band_number(given).map_err(failed)?,
        };
        if imported.bands.contains_key(&band) {
            return Err(failed(LineProblem::SameBand(band + 1)));
        }
        imported
            .bands
            .insert(band, filter_of(rest).map_err(failed)?);
        next_band = band + 1;
    }
    if imported.bands.is_empty() {
        return Err(ImportError::NoFilters);
    }
    Ok(imported)
}

fn note_ignored(imported: &mut Imported, command: &str, lower: &str) {
    let sounds = SOUND_COMMANDS.contains(&lower);
    let known = imported
        .ignored
        .iter()
        .any(|seen| seen.eq_ignore_ascii_case(command));
    if sounds && !known {
        imported.ignored.push(command.to_owned());
    }
}

/// Zero-based band from the one-based number after `Filter`.
fn band_number(given: &str) -> Result<usize, LineProblem> {
    let number: usize = given
        .parse()
        .map_err(|_| LineProblem::BadNumber(given.to_owned()))?;
    number
        .checked_sub(1)
        .ok_or_else(|| LineProblem::BadNumber(given.to_owned()))
}

/// `ON PK Fc 100 Hz Gain -3 dB Q 1.4`; `None` for a switched-off or empty filter.
fn filter_of(rest: &str) -> Result<Option<Filter>, LineProblem> {
    let tokens: Vec<&str> = rest.split_whitespace().collect();
    let Some((state, tokens)) = tokens.split_first() else {
        return Err(LineProblem::UnknownState(String::new()));
    };
    match state.to_ascii_uppercase().as_str() {
        "ON" => {}
        "OFF" => return Ok(None),
        _ => return Err(LineProblem::UnknownState((*state).to_owned())),
    }
    let Some((kind, parameters)) = tokens.split_first() else {
        return Err(LineProblem::UnknownType(String::new()));
    };
    let Some(file_type) = file_type(kind)? else {
        return Ok(None);
    };
    let values = Parameters::read(parameters)?;
    let frequency = values
        .frequency
        .ok_or(LineProblem::Missing(Parameter::Frequency))?;
    let mut boost = 0.0;
    if file_type.gain {
        boost = values.gain.ok_or(LineProblem::Missing(Parameter::Gain))?;
    }
    let q = values.q.ok_or(LineProblem::Missing(Parameter::Q))?;
    Ok(Some(Filter {
        kind: file_type.kind,
        frequency,
        boost,
        q,
    }))
}

#[derive(Default)]
struct Parameters {
    frequency: Option<f64>,
    gain: Option<f64>,
    q: Option<f64>,
}

impl Parameters {
    /// `Fc <f> Hz`, `Gain <g> dB`, `Q <q>` or `BW Oct <n>`, in any order; other words (units,
    /// Modal's `T60 target 100 ms`) are passed over.
    fn read(tokens: &[&str]) -> Result<Parameters, LineProblem> {
        let mut read = Parameters::default();
        let mut place = 0;
        while place < tokens.len() {
            let word = tokens[place].to_ascii_uppercase();
            let value_at = |offset: usize| tokens.get(place + offset).map(|token| number(token));
            match word.as_str() {
                "FC" => read.frequency = value_at(1).transpose()?,
                "GAIN" => read.gain = value_at(1).transpose()?,
                "Q" => read.q = value_at(1).transpose()?,
                "BW" if tokens
                    .get(place + 1)
                    .is_some_and(|unit| unit.eq_ignore_ascii_case("oct")) =>
                {
                    read.q = value_at(2).transpose()?.map(q_of_octaves);
                }
                _ => {}
            }
            place += 1;
        }
        Ok(read)
    }
}

/// A number with a point or a comma, a unit glued on or not (`129.1Hz`, `-3,5`).
fn number(token: &str) -> Result<f64, LineProblem> {
    let bare = token
        .trim_end_matches(|letter: char| letter.is_ascii_alphabetic())
        .replace(',', ".");
    bare.parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .ok_or_else(|| LineProblem::BadNumber(token.to_owned()))
}

/// The Q of a bandwidth in octaves (RBJ cookbook).
fn q_of_octaves(octaves: f64) -> f64 {
    let ratio = 2f64.powf(octaves);
    ratio.sqrt() / (ratio - 1.0)
}

/// The container after the import: the file's bands as given, every other band switched off
/// with its other settings kept. `current` is the container as the speaker reports it.
pub fn imported_preset(
    name: &str,
    imported: &Imported,
    current: &[Band],
) -> Result<EqPreset, ImportError> {
    let bands = current.len();
    if let Some(last) = imported
        .bands
        .keys()
        .next_back()
        .filter(|last| **last >= bands)
    {
        return Err(ImportError::TooManyBands {
            band: last + 1,
            bands,
        });
    }
    let result: Vec<Band> = current
        .iter()
        .enumerate()
        .map(|(place, band)| match imported.bands.get(&place) {
            Some(Some(filter)) => Band {
                enabled: true,
                gain: 0.0,
                boost: filter.boost,
                frequency: filter.frequency,
                q: filter.q,
                kind: filter.kind.to_owned(),
            },
            _ => Band {
                enabled: false,
                ..band.clone()
            },
        })
        .collect();
    let leaves = Leaf::ALL
        .iter()
        .map(|leaf| (leaf.name().to_owned(), leaf_array(*leaf, &result)))
        .collect();
    Ok(EqPreset {
        name: name.to_owned(),
        bands,
        leaves,
    })
}

fn leaf_array(leaf: Leaf, bands: &[Band]) -> Value {
    let entry = |band: &Band| match leaf {
        Leaf::Enabled => Value::Bool(band.enabled),
        Leaf::Gain => Value::from(band.gain),
        Leaf::Boost => Value::from(band.boost),
        Leaf::Frequency => Value::from(band.frequency),
        Leaf::Q => Value::from(band.q),
        Leaf::Type => Value::from(band.kind.clone()),
    };
    Value::Array(bands.iter().map(entry).collect())
}

/// A band the export could not write as it is (one-based numbers).
#[derive(Clone, Debug, PartialEq)]
pub enum ExportNote {
    Off(usize),
    /// The band's flat level, which APO filter lines cannot carry.
    FlatLevel {
        band: usize,
        gain: f64,
    },
    NotExportable {
        band: usize,
        kind: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Export {
    pub text: String,
    pub notes: Vec<ExportNote>,
}

/// APO's token for a speaker type, and whether its line carries a gain.
fn apo_type(kind: &str) -> Option<(&'static str, bool)> {
    match kind {
        "PARAMETRIC" => Some(("PK", true)),
        "LOWPASS" => Some(("LPQ", false)),
        "HIGHPASS" => Some(("HPQ", false)),
        "BANDPASS" => Some(("BP", false)),
        "NOTCH" => Some(("NO", false)),
        "ALLPASS" => Some(("AP", false)),
        _ => None,
    }
}

/// The bands as APO filter lines, each with its band number so a re-import puts it back in its
/// place; what has no APO line becomes a comment and a note.
pub fn export(bands: &[Band], container: &str) -> Export {
    let mut lines = vec![format!(
        "{COMMENT} Neumann Control EQ export: {container}, {} bands",
        bands.len()
    )];
    let mut notes = Vec::new();
    for (place, band) in bands.iter().enumerate() {
        let number = place + 1;
        if !band.enabled {
            lines.push(format!("{COMMENT} Filter {number}: off"));
            notes.push(ExportNote::Off(number));
            continue;
        }
        let Some((token, gain)) = apo_type(&band.kind) else {
            lines.push(format!(
                "{COMMENT} Filter {number}: {} not exported",
                band.kind
            ));
            notes.push(ExportNote::NotExportable {
                band: number,
                kind: band.kind.clone(),
            });
            continue;
        };
        lines.push(filter_line(number, token, gain, band));
        if band.gain != 0.0 {
            lines.push(format!(
                "{COMMENT} Filter {number}: flat level {} dB not exported",
                trimmed(band.gain)
            ));
            notes.push(ExportNote::FlatLevel {
                band: number,
                gain: band.gain,
            });
        }
    }
    lines.push(String::new());
    Export {
        text: lines.join("\r\n"),
        notes,
    }
}

fn filter_line(number: usize, token: &str, gain: bool, band: &Band) -> String {
    let mut line = format!(
        "Filter {number}: ON {token} Fc {} Hz",
        trimmed(band.frequency)
    );
    if gain {
        line.push_str(&format!(" Gain {} dB", trimmed(band.boost)));
    }
    line.push_str(&format!(" Q {}", trimmed(band.q)));
    line
}

/// Up to three decimals with a point, no trailing zeros and no `-0`.
fn trimmed(value: f64) -> String {
    let text = format!("{value:.3}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    match text {
        "-0" => "0".to_owned(),
        other => other.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn band(kind: &str, boost: f64, frequency: f64, q: f64) -> Band {
        Band {
            enabled: true,
            gain: 0.0,
            boost,
            frequency,
            q,
            kind: kind.to_owned(),
        }
    }

    fn filter(kind: &'static str, frequency: f64, boost: f64, q: f64) -> Option<Filter> {
        Some(Filter {
            kind,
            frequency,
            boost,
            q,
        })
    }

    fn problem_of(text: &str) -> LineProblem {
        match parse(text) {
            Err(ImportError::Line { problem, .. }) => problem,
            other => panic!("expected a line problem, got {other:?}"),
        }
    }

    #[test]
    fn an_apo_peak_line_becomes_a_parametric_band_with_its_gain_as_boost() {
        let imported = parse("Filter 1: ON PK Fc 100 Hz Gain -3.0 dB Q 1.41").unwrap();
        assert_eq!(imported.bands[&0], filter("PARAMETRIC", 100.0, -3.0, 1.41));
    }

    #[test]
    fn aligned_columns_glued_units_and_a_decimal_comma_are_read() {
        let text = "Filter  2: ON  PK       Fc   129,1Hz   Gain  -18,5dB  Q  4,00";
        let imported = parse(text).unwrap();
        assert_eq!(imported.bands[&1], filter("PARAMETRIC", 129.1, -18.5, 4.0));
    }

    #[test]
    fn filters_without_number_follow_the_previous_band() {
        let text = "Filter 3: ON NO Fc 50 Hz Q 2\nFilter: ON AP Fc 900 Hz Q 0.707\n";
        let imported = parse(text).unwrap();
        assert_eq!(imported.bands[&2], filter("NOTCH", 50.0, 0.0, 2.0));
        assert_eq!(imported.bands[&3], filter("ALLPASS", 900.0, 0.0, 0.707));
    }

    #[test]
    fn a_bandwidth_in_octaves_becomes_its_q() {
        let imported = parse("Filter: ON PEQ Fc 100 Hz Gain 1.0 dB BW Oct 1").unwrap();
        let q = imported.bands[&0].as_ref().unwrap().q;
        assert!((q - std::f64::consts::SQRT_2).abs() < 1e-9, "{q}");
    }

    #[test]
    fn none_and_off_switch_their_band_off() {
        let imported = parse("Filter 1: ON None\nFilter 2: OFF PK Fc 1 Hz").unwrap();
        assert_eq!(imported.bands[&0], None);
        assert_eq!(imported.bands[&1], None);
    }

    #[test]
    fn preamp_and_other_sound_commands_are_named_and_rew_headers_are_not() {
        let text = "\u{feff}Filter Settings file\r\nDated: 1-Jan-2026 10:00:00\r\nNotes:x\r\n\
                    Equaliser: Generic\r\nPreamp: -6 dB\r\nChannel: L\r\n# Filter 9: ON PK\r\n\
                    Filter 1: ON HPQ Fc 30 Hz Q 0.5\r\n";
        let imported = parse(text).unwrap();
        assert_eq!(imported.ignored, ["Preamp", "Channel"]);
        assert_eq!(imported.bands.len(), 1);
        assert_eq!(imported.bands[&0], filter("HIGHPASS", 30.0, 0.0, 0.5));
    }

    #[test]
    fn shelves_are_refused_until_measured() {
        for line in [
            "Filter: ON LSC Fc 80 Hz Gain 4 dB Q 0.7",
            "Filter: ON HS 6dB Fc 1 Hz",
        ] {
            assert!(
                matches!(problem_of(line), LineProblem::ShelfNotSupported(_)),
                "{line}"
            );
        }
    }

    #[test]
    fn an_unknown_type_refuses_the_whole_file_with_its_line() {
        let text = "Filter 1: ON PK Fc 100 Hz Gain 1 dB Q 1\nFilter 2: ON IIR Order 2";
        assert_eq!(
            parse(text),
            Err(ImportError::Line {
                line: 2,
                problem: LineProblem::UnknownType("IIR".to_owned()),
            })
        );
    }

    #[test]
    fn a_missing_q_or_gain_is_refused_rather_than_guessed() {
        assert_eq!(
            problem_of("Filter: ON LP Fc 8000 Hz"),
            LineProblem::Missing(Parameter::Q)
        );
        assert_eq!(
            problem_of("Filter: ON PK Fc 100 Hz Q 1"),
            LineProblem::Missing(Parameter::Gain)
        );
    }

    #[test]
    fn a_bad_number_and_a_second_line_for_a_band_are_refused() {
        assert_eq!(
            problem_of("Filter: ON PK Fc abc Hz Gain 1 dB Q 1"),
            LineProblem::BadNumber("abc".to_owned())
        );
        let twice = "Filter 1: ON None\nFilter 1: ON None";
        assert_eq!(problem_of(twice), LineProblem::SameBand(1));
    }

    #[test]
    fn a_file_without_filters_is_refused() {
        assert_eq!(parse("Preamp: -3 dB\n"), Err(ImportError::NoFilters));
    }

    #[test]
    fn the_import_switches_the_other_bands_off_and_keeps_their_settings() {
        let current = vec![
            band("NOTCH", 2.0, 50.0, 3.0),
            band("LOSHELF", 4.0, 80.0, 0.7),
        ];
        let imported = parse("Filter 1: ON PK Fc 100 Hz Gain -3 dB Q 1.4").unwrap();
        let preset = imported_preset("room.txt", &imported, &current).unwrap();
        assert_eq!(preset.bands, 2);
        assert_eq!(preset.leaves["enabled"], serde_json::json!([true, false]));
        assert_eq!(
            preset.leaves["type"],
            serde_json::json!(["PARAMETRIC", "LOSHELF"])
        );
        assert_eq!(preset.leaves["boost"], serde_json::json!([-3.0, 4.0]));
        assert_eq!(preset.leaves["gain"], serde_json::json!([0.0, 0.0]));
        assert_eq!(preset.leaves["q"], serde_json::json!([1.4, 0.7]));
    }

    #[test]
    fn more_bands_than_the_container_has_are_refused() {
        let current = vec![band("PARAMETRIC", 0.0, 100.0, 0.7); 2];
        let imported = parse("Filter 3: ON NO Fc 50 Hz Q 2").unwrap();
        assert_eq!(
            imported_preset("x", &imported, &current),
            Err(ImportError::TooManyBands { band: 3, bands: 2 })
        );
    }

    #[test]
    fn the_export_writes_numbered_apo_lines_and_comments_what_it_cannot() {
        let mut flat = band("PARAMETRIC", -3.0, 1230.0, 0.7);
        flat.gain = 1.5;
        let mut off = band("NOTCH", 0.0, 50.0, 2.0);
        off.enabled = false;
        let bands = vec![
            flat,
            off,
            band("LOSHELF", 4.0, 80.0, 0.7),
            band("HIGHPASS", 0.0, 30.5, 0.5),
        ];
        let export = export(&bands, "EQ 3");
        let lines: Vec<&str> = export.text.lines().collect();
        assert_eq!(
            lines,
            [
                "# Neumann Control EQ export: EQ 3, 4 bands",
                "Filter 1: ON PK Fc 1230 Hz Gain -3 dB Q 0.7",
                "# Filter 1: flat level 1.5 dB not exported",
                "# Filter 2: off",
                "# Filter 3: LOSHELF not exported",
                "Filter 4: ON HPQ Fc 30.5 Hz Q 0.5",
            ]
        );
        assert_eq!(
            export.notes,
            [
                ExportNote::FlatLevel { band: 1, gain: 1.5 },
                ExportNote::Off(2),
                ExportNote::NotExportable {
                    band: 3,
                    kind: "LOSHELF".to_owned()
                },
            ]
        );
    }

    #[test]
    fn an_exported_container_reads_back_as_the_same_bands() {
        let mut off = band("PARAMETRIC", 0.0, 100.0, 0.7);
        off.enabled = false;
        let bands = vec![
            band("PARAMETRIC", -4.5, 63.0, 2.5),
            off.clone(),
            band("LOWPASS", 0.0, 12000.0, 0.707),
            band("BANDPASS", 0.0, 500.0, 1.0),
            band("NOTCH", 0.0, 50.0, 8.0),
            band("ALLPASS", 0.0, 900.0, 0.7),
            band("HIGHPASS", 0.0, 25.0, 0.5),
        ];
        let text = export(&bands, "EQ 2").text;
        let imported = parse(&text).unwrap();
        let preset = imported_preset("back", &imported, &bands).unwrap();
        for leaf in Leaf::ALL {
            assert_eq!(
                preset.leaves[leaf.name()],
                leaf_array(leaf, &bands),
                "{leaf:?}"
            );
        }
    }

    #[test]
    fn exported_numbers_never_read_minus_zero() {
        assert_eq!(trimmed(-0.0001), "0");
        assert_eq!(trimmed(0.707), "0.707");
        assert_eq!(trimmed(24000.0), "24000");
    }
}
