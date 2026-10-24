//! EQ filters as text files: the filter lines of Equalizer APO's `config.txt`, which are also
//! what Room EQ Wizard exports for its "Generic" equaliser (`Filter 1: ON PK Fc 100 Hz Gain -3.0
//! dB Q 1.41`). Import maps them onto a container's bands; export writes a container as APO lines.
//! A shelf's Q in these files is the cookbook Q (read from the biquad REW stores), the speakers'
//! shelf `q` is the cookbook slope S (measured), so shelves are converted both ways.

use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::f64::consts::FRAC_1_SQRT_2;

use neumann_control_core::catalog::eq_type;
use neumann_control_core::settings::EqPreset;
use serde_json::Value;

use crate::eq::{Band, Leaf};
use crate::eq_response::{shelf_q_of_slope, shelf_slope_of_q};

/// Bigger than any filter file; a larger file is not one.
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;

const COMMENT: char = '#';
const FILTER_COMMAND: &str = "filter";
const BYTE_ORDER_MARK: char = '\u{feff}';
/// The second word of REW's two-word types (`LS Q`, `HP Q`): the variant with a Q.
const Q_SUFFIX: &str = "Q";
const FREQUENCY_KEY: &str = "FC";
/// REW's plain shelf (`LS`, `HS`), read from the biquad it stores: Q 1/√2, slope 1.
const PLAIN_SHELF_Q: f64 = FRAC_1_SQRT_2;
/// First-order filters have no Q; the speakers still want one in their range.
const FIRST_ORDER_Q: f64 = 0.7;
/// APO commands that start another channel's filters or a condition (lower case).
const CHANNEL_COMMAND: &str = "channel";
const CONDITION_COMMANDS: [&str; 3] = ["if", "elseif", "else"];
/// APO commands that change the sound but have no place in one EQ container; anything else
/// before a colon (REW's header lines `Dated:`, `Notes:` ...) is passed over silently.
const SOUND_COMMANDS: [&str; 11] = [
    "preamp",
    CHANNEL_COMMAND,
    "include",
    "graphiceq",
    "device",
    "delay",
    "copy",
    "convolution",
    "stage",
    "eval",
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
    /// Filters for more than one channel, or under a condition: one container takes one
    /// channel's filters, and stacking them all would sound like none of them.
    SeveralChannels,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LineProblem {
    UnknownType(String),
    UnsupportedForm(String),
    Missing(Parameter),
    BadNumber(String),
    UnknownState(String),
    /// A second line for the same band (one-based).
    SameBand(usize),
    /// A Q or bandwidth that gives no finite Q above 0.
    BadQ,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parameter {
    Frequency,
    Gain,
    Q,
}

/// What a file type's Q means for the speaker.
#[derive(Clone, Copy)]
enum QRule {
    /// The cookbook Q, taken as it is; a line without one is refused.
    Required,
    /// The cookbook Q of a shelf, converted to the speakers' slope; REW's plain shelf has none.
    Shelf,
    /// A first-order filter: no Q.
    Unused,
}

/// The speaker's type for a file type, whether its line carries a gain, and what its Q means.
struct FileType {
    kind: &'static str,
    gain: bool,
    q: QRule,
}

fn file_type(name: &str) -> Result<Option<FileType>, LineProblem> {
    let upper = name.to_ascii_uppercase();
    let shaped = |kind, gain, q| Ok(Some(FileType { kind, gain, q }));
    match upper.as_str() {
        "PK" | "PEQ" | "MODAL" | "PK Q" => shaped(eq_type::PARAMETRIC, true, QRule::Required),
        "LP" | "LPQ" | "LP Q" => shaped(eq_type::LOWPASS, false, QRule::Required),
        "HP" | "HPQ" | "HP Q" => shaped(eq_type::HIGHPASS, false, QRule::Required),
        "LP1" => shaped(eq_type::LO6DB, false, QRule::Unused),
        "HP1" => shaped(eq_type::HI6DB, false, QRule::Unused),
        "BP" => shaped(eq_type::BANDPASS, false, QRule::Required),
        "NO" => shaped(eq_type::NOTCH, false, QRule::Required),
        "AP" => shaped(eq_type::ALLPASS, false, QRule::Required),
        "LS" | "LSC" | "LS Q" => shaped(eq_type::LOSHELF, true, QRule::Shelf),
        "HS" | "HSC" | "HS Q" => shaped(eq_type::HISHELF, true, QRule::Shelf),
        "NONE" => Ok(None),
        _ => Err(LineProblem::UnknownType(name.to_owned())),
    }
}

/// The type's name and the words after it: REW writes `LS Q` / `HP Q` as two words. A shelf
/// followed by anything but `Fc` is APO's slope (`LSC 10.8 dB`) or corner (`LS 6dB`) form,
/// whose meaning is not verified.
fn type_and_parameters<'a>(tokens: &[&'a str]) -> Result<(String, Vec<&'a str>), LineProblem> {
    let Some((first, rest)) = tokens.split_first() else {
        return Err(LineProblem::UnknownType(String::new()));
    };
    let second = rest.first().copied().unwrap_or_default();
    let is_key = |word: &str| word.eq_ignore_ascii_case(FREQUENCY_KEY);
    let with_q =
        second.eq_ignore_ascii_case(Q_SUFFIX) && rest.get(1).is_some_and(|word| is_key(word));
    if with_q {
        return Ok((format!("{first} {Q_SUFFIX}"), rest[1..].to_vec()));
    }
    let shelf = matches!(
        first.to_ascii_uppercase().as_str(),
        "LS" | "HS" | "LSC" | "HSC"
    );
    if shelf && !second.is_empty() && !is_key(second) {
        return Err(LineProblem::UnsupportedForm(format!("{first} {second}")));
    }
    Ok(((*first).to_owned(), rest.to_vec()))
}

/// The filters of an APO or REW text. Filter lines with a number go to that band, lines
/// without one to the band after the previous line.
pub fn parse(text: &str) -> Result<Imported, ImportError> {
    let mut imported = Imported::default();
    let mut next_band = 0;
    let mut channels = 0;
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
        if CONDITION_COMMANDS.contains(&lower.as_str()) {
            return Err(ImportError::SeveralChannels);
        }
        if lower == CHANNEL_COMMAND {
            channels += 1;
            if channels > 1 {
                return Err(ImportError::SeveralChannels);
            }
        }
        let Some(number) = lower.strip_prefix(FILTER_COMMAND) else {
            note_ignored(&mut imported, command, &lower);
            continue;
        };
        let band = match number.trim() {
            "" => next_band,
            given => band_number(given).map_err(failed)?,
        };
        let filter = filter_of(rest).map_err(failed)?;
        next_band = band + 1;
        match imported.bands.entry(band) {
            Entry::Vacant(slot) => {
                slot.insert(filter);
            }
            // REW appends a second block of empty filters (`Filter 1: ON None` again).
            Entry::Occupied(_) if filter.is_none() => {}
            Entry::Occupied(_) => return Err(failed(LineProblem::SameBand(band + 1))),
        }
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
    let (name, parameters) = type_and_parameters(tokens)?;
    let Some(file_type) = file_type(&name)? else {
        return Ok(None);
    };
    let values = Parameters::read(&parameters)?;
    let frequency = values
        .frequency
        .ok_or(LineProblem::Missing(Parameter::Frequency))?;
    let mut boost = 0.0;
    if file_type.gain {
        boost = values.gain.ok_or(LineProblem::Missing(Parameter::Gain))?;
    }
    let q = match file_type.q {
        QRule::Required => values.q.ok_or(LineProblem::Missing(Parameter::Q))?,
        QRule::Shelf => shelf_slope_of_q(values.q.unwrap_or(PLAIN_SHELF_Q), boost),
        QRule::Unused => FIRST_ORDER_Q,
    };
    // A bandwidth of 0 octaves gives an infinite Q, which would reach the speaker as `null` and
    // leave the band's old Q in place.
    if !(q.is_finite() && q > 0.0) {
        return Err(LineProblem::BadQ);
    }
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
    // Empty slots past the container (REW writes all 20) are no filters.
    let beyond = imported
        .bands
        .iter()
        .filter(|(band, filter)| **band >= bands && filter.is_some())
        .map(|(band, _)| *band)
        .next_back();
    if let Some(last) = beyond {
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

/// APO's token for a band, whether its line carries a gain, and its cookbook Q (a shelf's slope
/// converted); `None` for a type APO has no line for, or a shelf beyond the cookbook form.
fn apo_line_of(band: &Band) -> Option<(&'static str, bool, f64)> {
    match band.kind.as_str() {
        eq_type::PARAMETRIC => Some(("PK", true, band.q)),
        eq_type::LOWPASS => Some(("LPQ", false, band.q)),
        eq_type::HIGHPASS => Some(("HPQ", false, band.q)),
        eq_type::BANDPASS => Some(("BP", false, band.q)),
        eq_type::NOTCH => Some(("NO", false, band.q)),
        eq_type::ALLPASS => Some(("AP", false, band.q)),
        eq_type::LOSHELF => Some(("LSC", true, shelf_q_of_slope(band.q, band.boost)?)),
        eq_type::HISHELF => Some(("HSC", true, shelf_q_of_slope(band.q, band.boost)?)),
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
        let Some((token, gain, q)) = apo_line_of(band) else {
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
        lines.push(filter_line(number, token, gain, q, band));
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

fn filter_line(number: usize, token: &str, gain: bool, q: f64, band: &Band) -> String {
    let mut line = format!(
        "Filter {number}: ON {token} Fc {} Hz",
        trimmed(band.frequency)
    );
    if gain {
        line.push_str(&format!(" Gain {} dB", trimmed(band.boost)));
    }
    line.push_str(&format!(" Q {}", trimmed(q)));
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

    /// A bandwidth of 0 octaves or a Q of 0 has no Q the speaker could take; it is refused
    /// instead of reaching the speaker as `null`, which would keep the band's old Q.
    #[test]
    fn a_q_of_zero_or_a_zero_bandwidth_is_refused() {
        for line in [
            "Filter: ON PK Fc 100 Hz Gain 1 dB BW Oct 0",
            "Filter: ON PK Fc 100 Hz Gain 1 dB Q 0",
            "Filter: ON PK Fc 100 Hz Gain 1 dB Q -1",
        ] {
            assert_eq!(problem_of(line), LineProblem::BadQ, "{line}");
        }
    }

    /// One EQ container holds one channel's filters; a file with several channels or with
    /// conditions would stack them into it.
    #[test]
    fn several_channels_or_conditions_are_refused() {
        let channels = "Channel: L
Filter: ON PK Fc 100 Hz Gain 1 dB Q 1
                        Channel: R
Filter: ON PK Fc 200 Hz Gain 1 dB Q 1
";
        assert_eq!(parse(channels), Err(ImportError::SeveralChannels));
        let condition = "If: sampleRate == 48000
Filter: ON PK Fc 100 Hz Gain 1 dB Q 1
";
        assert_eq!(parse(condition), Err(ImportError::SeveralChannels));
        let one = "Channel: L R
Filter: ON PK Fc 100 Hz Gain 1 dB Q 1
";
        assert!(parse(one).is_ok());
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

    /// Exported by Room EQ Wizard 5.31.3, equaliser "Generic" (2026-10-21): two-word types,
    /// all 20 slots, and a second block of two empty filters at the end.
    const REW_GENERIC_EXPORT: &str = "Filter Settings file\r\n\r\nRoom EQ V5.31.3\r\n\
        Dated: 21.10.2026 00:37:16\r\n\r\nNotes:testfilter\r\n\r\nEqualiser: Generic\r\n\
        No measurement\r\n\
        Filter  1: ON  PK       Fc   100.0 Hz  Gain  -3.00 dB  Q  1.410\r\n\
        Filter  2: ON  LS Q     Fc   200.0 Hz  Gain   6.00 dB Q   2.00\r\n\
        Filter  3: ON  HS Q     Fc    2000 Hz  Gain  -4.00 dB Q  0.700\r\n\
        Filter  4: ON  LS       Fc   100.0 Hz  Gain   3.00 dB\r\n\
        Filter  5: ON  HP Q     Fc   30.00 Hz  Q  0.700\r\n\
        Filter  6: ON  LP1      Fc   15.00 Hz \r\n\
        Filter  7: ON  None   \r\nFilter  8: ON  None   \r\nFilter  9: ON  None   \r\n\
        Filter 10: ON  None   \r\nFilter 11: ON  None   \r\nFilter 12: ON  None   \r\n\
        Filter 13: ON  None   \r\nFilter 14: ON  None   \r\nFilter 15: ON  None   \r\n\
        Filter 16: ON  None   \r\nFilter 17: ON  None   \r\nFilter 18: ON  None   \r\n\
        Filter 19: ON  None   \r\nFilter 20: ON  None   \r\n\
        Filter  1: ON  None      \r\nFilter  2: ON  None      \r\n\r\n";

    fn near(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-5, "{actual} vs {expected}");
    }

    #[test]
    fn a_real_rew_generic_export_reads_as_its_filters() {
        let imported = parse(REW_GENERIC_EXPORT).unwrap();
        assert!(imported.ignored.is_empty());
        assert_eq!(imported.bands.len(), 20);
        assert_eq!(imported.bands[&0], filter("PARAMETRIC", 100.0, -3.0, 1.41));
        assert_eq!(imported.bands[&4], filter("HIGHPASS", 30.0, 0.0, 0.7));
        let first_order = imported.bands[&5].as_ref().unwrap();
        assert_eq!((first_order.kind, first_order.frequency), ("LO6DB", 15.0));
        assert!((6..20).all(|band| imported.bands[&band].is_none()));
    }

    /// REW's shelf Q is the cookbook Q (read from the biquad it stores in a .req), the speakers'
    /// shelf q is the cookbook slope S (measured): Q 2 at +6 dB is q 5.72, REW's plain shelf
    /// (Q 0.7071) is q 1.
    #[test]
    fn a_rew_shelf_q_becomes_the_speakers_slope() {
        let imported = parse(REW_GENERIC_EXPORT).unwrap();
        let low = imported.bands[&1].as_ref().unwrap();
        assert_eq!(
            (low.kind, low.frequency, low.boost),
            ("LOSHELF", 200.0, 6.0)
        );
        near(low.q, 5.723559);
        let high = imported.bands[&2].as_ref().unwrap();
        assert_eq!(
            (high.kind, high.frequency, high.boost),
            ("HISHELF", 2000.0, -4.0)
        );
        near(high.q, 0.980509);
        let plain = imported.bands[&3].as_ref().unwrap();
        assert_eq!((plain.kind, plain.boost), ("LOSHELF", 3.0));
        near(plain.q, 1.0);
    }

    #[test]
    fn empty_slots_beyond_the_eq_are_ignored() {
        let current = vec![band("PARAMETRIC", 0.0, 100.0, 0.7); 10];
        let imported = parse(REW_GENERIC_EXPORT).unwrap();
        let preset = imported_preset("rew.txt", &imported, &current).unwrap();
        assert_eq!(preset.bands, 10);
    }

    #[test]
    fn shelves_by_slope_or_corner_frequency_are_refused() {
        for line in [
            "Filter: ON LSC 10.8 dB Fc 300 Hz Gain 5 dB",
            "Filter: ON HS 6dB Fc 1000 Hz Gain 3 dB",
        ] {
            assert!(
                matches!(problem_of(line), LineProblem::UnsupportedForm(_)),
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
        let twice = "Filter 1: ON None\nFilter 1: ON PK Fc 100 Hz Gain 1 dB Q 1";
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
                "Filter 3: ON LSC Fc 80 Hz Gain 4 dB Q 0.589",
                "Filter 4: ON HPQ Fc 30.5 Hz Q 0.5",
            ]
        );
        assert_eq!(
            export.notes,
            [
                ExportNote::FlatLevel { band: 1, gain: 1.5 },
                ExportNote::Off(2)
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
    fn an_exported_shelf_reads_back_as_the_same_slope() {
        let bands = vec![
            band("LOSHELF", 6.0, 200.0, 2.0),
            band("HISHELF", -4.0, 2000.0, 0.7),
        ];
        let imported = parse(&export(&bands, "EQ 2").text).unwrap();
        for (place, expected) in bands.iter().enumerate() {
            let back = imported.bands[&place].as_ref().unwrap();
            assert_eq!(back.kind, expected.kind);
            assert!(
                (back.q - expected.q).abs() < 0.01,
                "{} vs {}",
                back.q,
                expected.q
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
