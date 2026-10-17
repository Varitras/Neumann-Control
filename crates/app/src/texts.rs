//! German UI strings; the only place user-visible text lives.

use neumann_control_core::catalog::ValueError;

pub const UNREACHABLE: &str = "nicht erreichbar (aus oder Standby)";
pub const WRITE_FAILED: &str = "Schreiben fehlgeschlagen";
const SERIAL_MISMATCH_PREFIX: &str = "Anderes Gerät unter dieser Adresse (Seriennummer ";
const SERIAL_MISMATCH_SUFFIX: &str = ")";

const REJECTED_READ_ONLY_CODE: i64 = 405;
const REJECTED_WRONG_VALUE_CODE: i64 = 406;

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

pub fn describe_rejection(code: i64) -> String {
    match code {
        REJECTED_READ_ONLY_CODE => "Gerät lehnt ab: nur lesbar".to_owned(),
        REJECTED_WRONG_VALUE_CODE => "Gerät lehnt ab: falscher Wert".to_owned(),
        other => format!("Gerät lehnt ab (Code {other})"),
    }
}
