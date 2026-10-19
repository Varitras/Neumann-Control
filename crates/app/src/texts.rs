//! The app's UI strings in German and English, side by side so that no text lacks a translation.
//! Not here: group titles (`catalog::Group::title`), entry labels and units (`paths.toml`), and
//! the separators that format device values (`all_values`, `display`). Log texts stay English
//! and never come from here.

use std::cell::Cell;
use std::fmt;

use eframe::egui::{RichText, WidgetText};
use neumann_control_core::address::AddressError;
use neumann_control_core::catalog::ValueError;
use neumann_control_core::settings::Language;

use crate::state::Message;
use crate::writes::Refusal;

thread_local! {
    /// UI texts are only read on the UI thread; per thread rather than per process so that UI
    /// tests running in parallel each keep their own language.
    static LANGUAGE: Cell<Language> = const { Cell::new(Language::German) };
}

/// The language every text of this thread is read in from now on.
pub fn set_language(language: Language) {
    LANGUAGE.with(|current| current.set(language));
}

pub fn language() -> Language {
    LANGUAGE.with(Cell::get)
}

fn pick<T>(german: T, english: T) -> T {
    match language() {
        Language::German => german,
        Language::English => english,
    }
}

/// A number as written in the language: a decimal comma in German, a point in English. Typed
/// values accept both (`gesture::typed_value`).
pub fn number_text(text: String) -> String {
    match language() {
        Language::German => text.replace('.', ","),
        Language::English => text,
    }
}

pub fn one_decimal(value: f64) -> String {
    number_text(format!("{value:.1}"))
}

pub fn two_decimals(value: f64) -> String {
    number_text(format!("{value:.2}"))
}

/// The shortest text that reads back as exactly `value`.
pub fn number(value: f64) -> String {
    number_text(value.to_string())
}

/// One UI text in both languages.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Text {
    german: &'static str,
    english: &'static str,
}

impl Text {
    pub const fn new(german: &'static str, english: &'static str) -> Text {
        Text { german, english }
    }

    /// The text in the current language.
    pub fn get(self) -> &'static str {
        pick(self.german, self.english)
    }
}

impl fmt::Display for Text {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.get())
    }
}

impl From<Text> for WidgetText {
    fn from(text: Text) -> WidgetText {
        text.get().into()
    }
}

impl From<Text> for RichText {
    fn from(text: Text) -> RichText {
        RichText::new(text.get())
    }
}

impl From<Text> for String {
    fn from(text: Text) -> String {
        text.get().to_owned()
    }
}

pub const WINDOW_TITLE: &str = "Neumann Control";
pub const SCAN: Text = Text::new("Suchen", "Scan");
pub const ADD_ADDRESS: Text = Text::new("+ Adresse", "+ Address");
pub const SCAN_FAILED: Text = Text::new("Suche fehlgeschlagen", "Scan failed");
pub const SETTINGS_NOT_SAVED: Text = Text::new(
    "Einstellungen konnten nicht gespeichert werden",
    "Settings could not be saved",
);
pub const NO_SPEAKERS: Text = Text::new(
    "Keine Lautsprecher. „Suchen“ findet sie im Netz, „+ Adresse“ fügt einen von Hand hinzu.",
    "No speakers. “Scan” finds them on the network, “+ Address” adds one by hand.",
);

pub const MUTE: Text = Text::new("Stumm", "Mute");
pub const IDENTIFY: Text = Text::new("Ident.", "Ident.");
pub const LEVEL: Text = Text::new("Pegel", "Level");
pub const CLIP: Text = Text::new("Übersteuert", "Clipping");
pub const LED_CONNECTION: Text = Text::new("Verbindung", "Connection");
pub const LED_CLIP: Text = Text::new("Übersteuerung", "Clipping");
pub const LED_WARNING: Text = Text::new("Warnung des Geräts", "Device warning");
pub const LED_LOCAL: Text = Text::new(
    "LOCAL: Schalter hinten, Schreiben gesperrt",
    "LOCAL: rear switch, writing locked",
);
pub const CLOSE: Text = Text::new("Schließen", "Close");
pub const ADD_GROUP: Text = Text::new("+ Gruppe", "+ Group");
pub const MOVE_STRIP: Text = Text::new("Verschieben", "Move");
pub const GROUP_DIALOG_TITLE: Text = Text::new("Gruppe", "Group");
pub const GROUP_NAME: Text = Text::new("Name", "Name");
pub const DELETE_GROUP: Text = Text::new("Gruppe löschen", "Delete group");
pub const NEW_GROUP_NAME: Text = Text::new("Gruppe", "Group");
pub const GROUP: Text = Text::new("Gruppe", "Group");
pub const NO_GROUP: Text = Text::new("keine", "none");
pub const LINKED: Text = Text::new("Gekoppelt", "Linked");
pub const COPY_TO_GROUP: Text = Text::new("Für Gruppe übernehmen", "Apply to group");
pub const MASTER: Text = Text::new("Master", "Master");
pub const MASTER_LEVEL: Text = Text::new("Master-Pegel", "Master level");
pub const MUTE_ALL: Text = Text::new("Alle stumm", "Mute all");
/// A speaker of a history entry that has been removed since.
pub const REMOVED_SPEAKER: Text = Text::new("entfernter Lautsprecher", "removed speaker");
pub const CONTROL_HINT: Text = Text::new(
    "Shift: fein · Doppelklick: Wert eingeben",
    "Shift: fine · Double-click: type a value",
);
/// Faders take no typed value themselves (the strip's readout below does).
pub const FADER_HINT: Text = Text::new("Shift: fein", "Shift: fine");
pub const PANEL_TITLE: Text = Text::new("Lautsprecher", "Speaker");
pub const SETTINGS: Text = Text::new("Einstellungen", "Settings");
pub const COMPACT: Text = Text::new("Kompakt", "Compact");
pub const COMPACT_HINT: Text = Text::new("Strg+K", "Ctrl+K");
pub const SETTINGS_TITLE: Text = Text::new("Einstellungen", "Settings");
pub const THEME: Text = Text::new("Darstellung", "Appearance");
pub const THEME_SYSTEM: Text = Text::new("wie Windows", "like Windows");
pub const THEME_LIGHT: Text = Text::new("Hell", "Light");
pub const THEME_DARK: Text = Text::new("Dunkel", "Dark");
pub const LANGUAGE_LABEL: Text = Text::new("Sprache", "Language");
/// Each language is named in itself, so it can be found from either.
pub const LANGUAGE_GERMAN: &str = "Deutsch";
pub const LANGUAGE_ENGLISH: &str = "English";
pub const METER_RANGE: Text = Text::new("Pegelanzeige ab", "Meters from");
pub const SHOW_RESPONSE_TIMES: Text = Text::new("Antwortzeiten anzeigen", "Show response times");
pub const INPUT_METERS: Text = Text::new("Eingangspegel", "Input level");
pub const OUTPUT_METERS: Text = Text::new("Ausgangspegel", "Output level");
pub const METER_INPUT_SHORT: Text = Text::new("Eing.", "In");
pub const METER_OUTPUT_SHORT: Text = Text::new("Ausg.", "Out");
pub const SOURCE_ANALOG: Text = Text::new("Analog", "Analog");
pub const SOURCE_AES3_LEFT: Text = Text::new("AES3 L", "AES3 L");
pub const SOURCE_AES3_RIGHT: Text = Text::new("AES3 R", "AES3 R");
pub const SOURCE_MONO: Text = Text::new("Mono (A+B)", "Mono (A+B)");
pub const PLAYED_CHANNEL: Text = Text::new(
    "Wird wiedergegeben (Annahme: A = links, B = rechts)",
    "Played (assumed: A = left, B = right)",
);

pub const ALL_VALUES: Text = Text::new("Alle Werte…", "All values…");
pub const APPLY: Text = Text::new("Übernehmen", "Apply");
pub const ON: Text = Text::new("an", "on");
pub const OFF: Text = Text::new("aus", "off");
pub const CELSIUS: &str = "°C";
pub const MILLISECONDS: &str = "ms";

pub const ALL_VALUES_FILTER_LABEL: Text = Text::new("Pfad enthält", "Path contains");
pub const ALL_VALUES_RELOAD: Text = Text::new("Neu einlesen", "Read again");
pub const ALL_VALUES_LOADING: Text = Text::new("Lese alle Werte …", "Reading all values …");
pub const ALL_VALUES_FAILED: Text = Text::new(
    "Die Werte konnten nicht gelesen werden. Das Gerät ist nicht erreichbar oder hat die Abfrage abgelehnt.",
    "The values could not be read. The device is unreachable or refused the request.",
);
pub const ALL_VALUES_PATH_COLUMN: Text = Text::new("Pfad", "Path");
pub const ALL_VALUES_VALUE_COLUMN: Text = Text::new("Wert", "Value");
pub const ALL_VALUES_RANGE_COLUMN: Text = Text::new("Einheit / Bereich", "Unit / range");
pub const ALL_VALUES_KNOWN_COLUMN: Text = Text::new("Katalog", "Catalog");
pub const ALL_VALUES_KNOWN: Text = Text::new("bekannt", "known");
pub const ALL_VALUES_COPY: Text = Text::new("Kopieren", "Copy");

pub const ADD_ADDRESS_TITLE: Text = Text::new(
    "Lautsprecher per Adresse hinzufügen",
    "Add a speaker by address",
);
pub const ADDRESS_FIELD: Text = Text::new("IPv6-Adresse", "IPv6 address");
pub const ADDRESS_HINT: Text = Text::new("z. B. fe80::1%7", "e.g. fe80::1%7");
pub const ADD: Text = Text::new("Hinzufügen", "Add");
pub const CANCEL: Text = Text::new("Abbrechen", "Cancel");
pub const ADDRESS_ALREADY_LISTED: Text = Text::new(
    "Diese Adresse ist schon eingetragen.",
    "This address is already listed.",
);

pub const SCAN_CHOICE_TITLE: Text = Text::new("Gefundene Lautsprecher", "Speakers found");
pub const NOTHING_FOUND: Text = Text::new("Keine Lautsprecher gefunden", "No speakers found");
pub const ALREADY_ADDED: Text = Text::new("bereits hinzugefügt", "already added");

pub const REMOVE: Text = Text::new("Entfernen", "Remove");
pub const REMOVE_TITLE: Text = Text::new("Lautsprecher entfernen?", "Remove speaker?");

pub const UNREACHABLE: Text = Text::new(
    "nicht erreichbar (aus oder Standby)",
    "unreachable (off or standby)",
);
pub const WRITE_FAILED: Text = Text::new("Schreiben fehlgeschlagen", "Write failed");

const REJECTED_LOCAL_MODE_CODE: i64 = 401;
const REJECTED_READ_ONLY_CODE: i64 = 405;
const REJECTED_WRONG_VALUE_CODE: i64 = 406;

pub fn speaker_count(count: usize) -> String {
    pick(
        format!("{count} Lautsprecher"),
        match count {
            1 => format!("{count} speaker"),
            _ => format!("{count} speakers"),
        },
    )
}

pub fn all_values_title(name: &str) -> String {
    pick(
        format!("Alle Werte – {name}"),
        format!("All values – {name}"),
    )
}

pub fn all_values_count(shown: usize, total: usize) -> String {
    pick(
        format!("{shown} von {total} Werten"),
        format!("{shown} of {total} values"),
    )
}

pub fn port(port: u16) -> String {
    format!("Port {port}")
}

/// The channels as the speakers describe their meters: inputs "[A,B]", the KH 750's outputs
/// "[A, B, subwoofer]".
pub const INPUT_CHANNELS: &[&str] = &["A", "B"];
pub const OUTPUT_CHANNELS: &[&str] = &["A", "B", "Sub"];

/// The name under a meter; a channel the speakers never described is counted from 1.
pub fn channel_name(names: &[&str], index: usize) -> String {
    names
        .get(index)
        .map_or_else(|| (index + 1).to_string(), |name| (*name).to_owned())
}

/// Names one meter for screen readers.
pub fn meter_label(group: &str, channel: &str) -> String {
    format!("{group} {channel}")
}

/// The number under a meter: whole dB with a real minus sign, a dash when below the floor.
pub fn meter_readout(db: Option<i32>) -> String {
    match db {
        Some(db) if db < 0 => format!("\u{2212}{}", db.unsigned_abs()),
        Some(db) => db.to_string(),
        None => "\u{2013}".to_owned(),
    }
}

/// The panel's knob beside a number field, named apart from the field.
pub fn knob_label(label: &str) -> String {
    pick(format!("{label} (Regler)"), format!("{label} (knob)"))
}

/// A mark on the meter scale, without unit to fit beside the meters.
pub fn scale_mark(db: f64) -> String {
    format!("{db}")
}

pub fn meter_floor(db: f64) -> String {
    format!("{db} dB")
}

/// The toolbar's average over all connected speakers.
pub fn average_response_time(milliseconds: u128) -> String {
    format!("Ø {milliseconds} ms")
}

/// Names a group's link button for screen readers, apart from the strips' own buttons.
pub fn link_label(name: &str) -> String {
    pick(format!("{name} koppeln"), format!("Link {name}"))
}

pub fn copy_to_group(setting: &str) -> String {
    format!("{COPY_TO_GROUP}: {setting}")
}

pub fn group_level(group: &str) -> String {
    pick(format!("Pegel {group}"), format!("Level {group}"))
}

pub fn group_mute(group: &str) -> String {
    pick(format!("{group} stumm"), format!("Mute {group}"))
}

pub fn response_time(milliseconds: u128) -> String {
    format!("{milliseconds} ms")
}

/// How far a group or master fader has moved while it is dragged.
pub fn level_move(moved: f64) -> String {
    number_text(format!("{moved:+.1} dB"))
}

/// A group's or the master's level, without unit: it moves several speakers by the same amount.
pub fn shared_readout(value: f64) -> String {
    one_decimal(value)
}

pub fn readout(value: f64, unit: &str) -> String {
    format!("{} {unit}", one_decimal(value))
}

pub fn status_line(firmware: &str, masked_serial: &str) -> String {
    format!("Firmware {firmware} · SN {masked_serial}")
}

pub fn describe_address_error(error: &AddressError) -> String {
    let text = match error {
        AddressError::Ipv4NotSupported => Text::new(
            "IPv4 geht nicht: die Lautsprecher sind nur über IPv6 erreichbar, z. B. fe80::1%7.",
            "IPv4 does not work: the speakers are only reachable over IPv6, e.g. fe80::1%7.",
        ),
        AddressError::MissingScope => Text::new(
            "Bei fe80-Adressen fehlt der Schnittstellenindex nach dem %, z. B. fe80::1%7.",
            "An fe80 address needs the interface index after the %, e.g. fe80::1%7.",
        ),
        AddressError::ScopeNotNumeric => Text::new(
            "Nach dem % muss der Schnittstellenindex als Zahl stehen.",
            "The interface index after the % must be a number.",
        ),
        AddressError::Invalid => {
            Text::new("Keine gültige IPv6-Adresse.", "Not a valid IPv6 address.")
        }
    };
    text.get().to_owned()
}

pub fn describe_message(message: &Message) -> String {
    match message {
        Message::Unreachable => UNREACHABLE.into(),
        Message::WriteFailed => WRITE_FAILED.into(),
        Message::Rejected(code) => describe_rejection(*code),
        Message::SerialMismatch(masked_serial) => serial_mismatch(masked_serial),
        Message::Refused(refusal) => describe_refusal(refusal),
    }
}

/// A group row's note about one of its members.
pub fn member_message(name: &str, message: &str) -> String {
    format!("{name}: {message}")
}

pub fn serial_mismatch(masked_serial: &str) -> String {
    pick(
        format!("Anderes Gerät unter dieser Adresse (Seriennummer {masked_serial})"),
        format!("Another device at this address (serial number {masked_serial})"),
    )
}

pub fn describe_value_error(error: &ValueError) -> String {
    match error {
        ValueError::OutOfRange { min, max } => pick(
            format!("Wert außerhalb von {} bis {}.", number(*min), number(*max)),
            format!("Value outside {} to {}.", number(*min), number(*max)),
        ),
        ValueError::NotAnOption => Text::new(
            "Wert ist keine der Auswahlmöglichkeiten.",
            "Value is not one of the choices.",
        )
        .into(),
        ValueError::NotAStep => Text::new(
            "Wert passt nicht zur Schrittweite.",
            "Value does not fit the step size.",
        )
        .into(),
        ValueError::TooLong(limit) => pick(
            format!("Höchstens {limit} Zeichen."),
            format!("At most {limit} characters."),
        ),
        ValueError::NotPrintableAscii => Text::new(
            "Nur Buchstaben A–Z, Ziffern und Satzzeichen ohne Umlaute.",
            "Only letters A–Z, digits and punctuation, no accented letters.",
        )
        .into(),
        ValueError::WrongType => {
            Text::new("Wert hat den falschen Typ.", "Value has the wrong type.").into()
        }
    }
}

pub fn describe_refusal(refusal: &Refusal) -> String {
    match refusal {
        Refusal::NotConnected => Text::new("Nicht verbunden", "Not connected").into(),
        Refusal::NotWritable => Text::new(
            "Für dieses Modell nicht freigegeben",
            "Not enabled for this model",
        )
        .into(),
        Refusal::ValueUnknown => Text::new("Wert noch unbekannt", "Value not known yet").into(),
        Refusal::Invalid(error) => describe_value_error(error),
    }
}

pub fn describe_rejection(code: i64) -> String {
    match code {
        REJECTED_LOCAL_MODE_CODE => Text::new(
            "Gerät steht auf LOCAL (Schalter hinten), Schreiben gesperrt",
            "Device is set to LOCAL (rear switch), writing locked",
        )
        .into(),
        REJECTED_READ_ONLY_CODE => {
            Text::new("Gerät lehnt ab: nur lesbar", "Device refuses: read only").into()
        }
        REJECTED_WRONG_VALUE_CODE => Text::new(
            "Gerät lehnt ab: falscher Wert",
            "Device refuses: wrong value",
        )
        .into(),
        other => pick(
            format!("Gerät lehnt ab (Code {other})"),
            format!("Device refuses (code {other})"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_text_follows_the_language_of_its_thread() {
        assert_eq!(MUTE.get(), "Stumm");
        set_language(Language::English);
        assert_eq!(MUTE.get(), "Mute");
        assert_eq!(speaker_count(1), "1 speaker");
        set_language(Language::German);
        assert_eq!(MUTE.to_string(), "Stumm");
        assert_eq!(readout(87.5, "dB SPL"), "87,5 dB SPL");
        set_language(Language::English);
        assert_eq!(readout(87.5, "dB SPL"), "87.5 dB SPL");
        set_language(Language::German);
    }
}
