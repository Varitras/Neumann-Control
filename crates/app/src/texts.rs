//! The app's German UI strings. Not here: group titles (`catalog::Group::title`), entry labels
//! and units (`paths.toml`), and the separators that format device values (`all_values`,
//! `display`).

use neumann_control_core::address::AddressError;
use neumann_control_core::catalog::ValueError;

use crate::writes::Refusal;

pub const WINDOW_TITLE: &str = "Neumann Control";
pub const SCAN: &str = "Suchen";
pub const ADD_ADDRESS: &str = "+ Adresse";
pub const SCAN_FAILED: &str = "Suche fehlgeschlagen";
pub const SETTINGS_NOT_SAVED: &str = "Einstellungen konnten nicht gespeichert werden";
pub const NO_SPEAKERS: &str =
    "Keine Lautsprecher. „Suchen“ findet sie im Netz, „+ Adresse“ fügt einen von Hand hinzu.";

pub const MUTE: &str = "Stumm";
pub const IDENTIFY: &str = "Ident.";
pub const MORE: &str = "Mehr";
pub const LESS: &str = "Weniger";
pub const CONNECTING: &str = "verbinde …";
pub const CONNECTED: &str = "verbunden";
pub const LEVEL: &str = "Pegel";
pub const CLIP: &str = "Übersteuert";

pub const ALL_VALUES: &str = "Alle Werte…";
pub const APPLY: &str = "Übernehmen";
pub const ON: &str = "an";
pub const OFF: &str = "aus";
pub const CELSIUS: &str = "°C";

pub const ALL_VALUES_FILTER_LABEL: &str = "Pfad enthält";
pub const ALL_VALUES_RELOAD: &str = "Neu einlesen";
pub const ALL_VALUES_LOADING: &str = "Lese alle Werte …";
pub const ALL_VALUES_FAILED: &str =
    "Die Werte konnten nicht gelesen werden. Das Gerät ist nicht erreichbar oder hat die Abfrage abgelehnt.";
pub const ALL_VALUES_PATH_COLUMN: &str = "Pfad";
pub const ALL_VALUES_VALUE_COLUMN: &str = "Wert";
pub const ALL_VALUES_RANGE_COLUMN: &str = "Einheit / Bereich";
pub const ALL_VALUES_KNOWN_COLUMN: &str = "Katalog";
pub const ALL_VALUES_KNOWN: &str = "bekannt";
pub const ALL_VALUES_COPY: &str = "Kopieren";

pub const ADD_ADDRESS_TITLE: &str = "Lautsprecher per Adresse hinzufügen";
pub const ADDRESS_FIELD: &str = "IPv6-Adresse";
pub const ADDRESS_HINT: &str = "z. B. fe80::1%7";
pub const ADD: &str = "Hinzufügen";
pub const CANCEL: &str = "Abbrechen";
pub const ADDRESS_ALREADY_LISTED: &str = "Diese Adresse ist schon eingetragen.";

pub const SCAN_CHOICE_TITLE: &str = "Gefundene Lautsprecher";
pub const NOTHING_FOUND: &str = "Keine Lautsprecher gefunden";
pub const ALREADY_ADDED: &str = "bereits hinzugefügt";

pub const REMOVE: &str = "Entfernen";
pub const REMOVE_TITLE: &str = "Lautsprecher entfernen?";

pub const UNREACHABLE: &str = "nicht erreichbar (aus oder Standby)";
pub const WRITE_FAILED: &str = "Schreiben fehlgeschlagen";
const SERIAL_MISMATCH_PREFIX: &str = "Anderes Gerät unter dieser Adresse (Seriennummer ";
const SERIAL_MISMATCH_SUFFIX: &str = ")";

const REJECTED_LOCAL_MODE_CODE: i64 = 401;
const REJECTED_READ_ONLY_CODE: i64 = 405;
const REJECTED_WRONG_VALUE_CODE: i64 = 406;

pub fn speaker_count(count: usize) -> String {
    format!("{count} Lautsprecher")
}

pub fn all_values_title(name: &str) -> String {
    format!("Alle Werte – {name}")
}

pub fn all_values_count(shown: usize, total: usize) -> String {
    format!("{shown} von {total} Werten")
}

pub fn port(port: u16) -> String {
    format!("Port {port}")
}

pub fn readout(value: f64, unit: &str) -> String {
    format!("{value:.1} {unit}")
}

pub fn status_line(firmware: &str, masked_serial: &str) -> String {
    format!("Firmware {firmware} · SN {masked_serial}")
}

pub fn describe_address_error(error: &AddressError) -> String {
    let text = match error {
        AddressError::Ipv4NotSupported => {
            "IPv4 geht nicht: die Lautsprecher sind nur über IPv6 erreichbar, z. B. fe80::1%7."
        }
        AddressError::MissingScope => {
            "Bei fe80-Adressen fehlt der Schnittstellenindex nach dem %, z. B. fe80::1%7."
        }
        AddressError::ScopeNotNumeric => "Nach dem % muss der Schnittstellenindex als Zahl stehen.",
        AddressError::Invalid => "Keine gültige IPv6-Adresse.",
    };
    text.to_owned()
}

pub fn serial_mismatch(masked_serial: &str) -> String {
    format!("{SERIAL_MISMATCH_PREFIX}{masked_serial}{SERIAL_MISMATCH_SUFFIX}")
}

pub fn describe_value_error(error: &ValueError) -> String {
    match error {
        ValueError::OutOfRange { min, max } => format!("Wert außerhalb von {min} bis {max}."),
        ValueError::NotAnOption => "Wert ist keine der Auswahlmöglichkeiten.".to_owned(),
        ValueError::NotAStep => "Wert passt nicht zur Schrittweite.".to_owned(),
        ValueError::TooLong(limit) => format!("Höchstens {limit} Zeichen."),
        ValueError::NotPrintableAscii => {
            "Nur Buchstaben A–Z, Ziffern und Satzzeichen ohne Umlaute.".to_owned()
        }
        ValueError::WrongType => "Wert hat den falschen Typ.".to_owned(),
    }
}

pub fn describe_refusal(refusal: &Refusal) -> String {
    match refusal {
        Refusal::NotConnected => "Nicht verbunden".to_owned(),
        Refusal::NotWritable => "Für dieses Modell nicht freigegeben".to_owned(),
        Refusal::ValueUnknown => "Wert noch unbekannt".to_owned(),
        Refusal::Invalid(error) => describe_value_error(error),
    }
}

pub fn describe_rejection(code: i64) -> String {
    match code {
        REJECTED_LOCAL_MODE_CODE => {
            "Gerät steht auf LOCAL (Schalter hinten), Schreiben gesperrt".to_owned()
        }
        REJECTED_READ_ONLY_CODE => "Gerät lehnt ab: nur lesbar".to_owned(),
        REJECTED_WRONG_VALUE_CODE => "Gerät lehnt ab: falscher Wert".to_owned(),
        other => format!("Gerät lehnt ab (Code {other})"),
    }
}
