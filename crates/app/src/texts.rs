//! The app's UI strings in German and English, side by side so that no text lacks a translation.
//! Not here: group titles (`catalog::Group::title`), entry labels and units (`paths.toml`), and
//! the separators that format device values in the panel and "Alle Werte" (`all_values`,
//! `display`); the history's own value line is here (`history_value`). Log texts stay English
//! and never come from here.

use std::cell::Cell;
use std::fmt;

use eframe::egui::{RichText, WidgetText};
use neumann_control_core::address::AddressError;
use neumann_control_core::catalog::{Group, ValueError};
use neumann_control_core::settings::{Language, LoadOutcome};
use neumann_control_core::ssc::wire::DeviceError;
use serde_json::Value;

use crate::eq_file::{ExportNote, ImportError, LineProblem, Parameter};
use crate::state::{Message, ScanFailure};
use crate::trend::whole_minutes;
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
    // A value that shows as zero has no sign: the speakers report -0.0, and -0.04 rounds to "-0.0".
    let shows_zero = text.strip_prefix('-').is_some_and(|rest| {
        rest.chars()
            .all(|character| character == '0' || character == '.')
    });
    let text = if shows_zero {
        text[1..].to_owned()
    } else {
        text
    };
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
pub const SETTINGS_PARTLY_UNDERSTOOD: Text = Text::new(
    "Einstellungen teils nicht verstanden, Vorgaben genommen; Kopie liegt als .bad daneben",
    "Some settings not understood, defaults taken; a copy is kept as .bad",
);
pub const SETTINGS_SET_ASIDE: Text = Text::new(
    "Einstellungsdatei unlesbar, als .bad beiseitegelegt; Vorgaben genommen",
    "Settings file unreadable, moved aside as .bad; defaults taken",
);
pub const SETTINGS_UNTOUCHABLE: Text = Text::new(
    "Einstellungsdatei nicht lesbar; Änderungen werden nicht gespeichert",
    "Settings file cannot be read; changes are not saved",
);
pub const NO_SPEAKERS: Text = Text::new(
    "Keine Lautsprecher. „Suchen“ findet sie im Netz, „+ Adresse“ fügt einen von Hand hinzu.",
    "No speakers. “Scan” finds them on the network, “+ Address” adds one by hand.",
);

pub const MUTE: Text = Text::new("Stumm", "Mute");
pub const IDENTIFY: Text = Text::new("Ident.", "Ident.");
/// The KH 750's outputs A and B on a strip, beside its own mute.
pub const MUTE_OUTPUT_A: Text = Text::new("Stumm A", "Mute A");
pub const MUTE_OUTPUT_B: Text = Text::new("Stumm B", "Mute B");
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
pub const HELP: Text = Text::new("?", "?");
pub const HELP_HINT: Text = Text::new("Hilfe", "Help");
pub const MANUAL: Text = Text::new("Handbuch", "Manual");
pub const WIKI: Text = Text::new("Wiki", "Wiki");
pub const ABOUT: Text = Text::new("Über Neumann Control", "About Neumann Control");
pub const MANUAL_MISSING: Text = Text::new(
    "Handbuch nicht gefunden (neben der Exe)",
    "Manual not found (next to the exe)",
);
pub const LICENCE: Text = Text::new("MIT-Lizenz", "MIT licence");
pub const PROJECT_PAGE: Text = Text::new("Projektseite auf GitHub", "Project page on GitHub");
pub const CHECK_FOR_UPDATES: Text = Text::new("Nach Updates suchen", "Check for updates");
pub const UP_TO_DATE: Text = Text::new("Diese Version ist aktuell.", "This version is up to date.");
pub const NO_RELEASE: Text = Text::new(
    "Noch kein Release veröffentlicht.",
    "No release published yet.",
);
pub const UPDATE_CHECK_FAILED: Text = Text::new(
    "GitHub war nicht erreichbar.",
    "GitHub could not be reached.",
);
pub const UPDATE_CHECK_UNUSABLE: Text = Text::new(
    "GitHub hat geantwortet, die Antwort ließ sich aber nicht auswerten.",
    "GitHub answered, but the answer could not be read.",
);
pub const OPEN_RELEASE_PAGE: Text = Text::new("Release-Seite öffnen", "Open the release page");
pub const ADD_GROUP: Text = Text::new("+ Gruppe", "+ Group");
pub const MOVE_STRIP: Text = Text::new("Verschieben", "Move");
pub const HIDE: Text = Text::new("Ausblenden", "Hide");
pub const SWAP_FOR: Text = Text::new("Tauschen gegen…", "Swap for…");
pub const MOVE_UP: Text = Text::new("Nach oben", "Move up");
pub const MOVE_DOWN: Text = Text::new("Nach unten", "Move down");
pub const MOVE_LEFT: Text = Text::new("Nach links", "Move left");
pub const MOVE_RIGHT: Text = Text::new("Nach rechts", "Move right");
pub const ADD_BUTTON: Text = Text::new("Hinzufügen…", "Add…");
pub const ADD_TO_STRIP: Text = Text::new("Auf den Zug", "Add to strip");
pub const REMOVE_FROM_STRIP: Text = Text::new("Vom Zug nehmen", "Remove from strip");
pub const CUSTOMIZE_STRIPS: Text = Text::new("Züge anpassen", "Customize strips");
pub const DONE: Text = Text::new("Fertig", "Done");
pub const ADD_ELEMENT: Text = Text::new("+ hinzufügen", "+ Add");
pub const DEFAULT_LAYOUT: Text = Text::new("Standard", "Default");
pub const GROUP_DIALOG_TITLE: Text = Text::new("Gruppe", "Group");
pub const GROUP_NAME: Text = Text::new("Name", "Name");
pub const GROUP_MEMBERS: Text = Text::new("Lautsprecher", "Speakers");
pub const STRIP_COLOR: Text = Text::new("Farbe des Zugs", "Strip colour");
pub const COLOR_DEFAULT: Text = Text::new("Standard", "Default");
pub const COLOR_LIKE_GROUP: Text = Text::new("Wie die Gruppe", "As the group");
pub const COLOR_OFF: Text = Text::new("Keine", "None");
pub const COLOR_OWN: Text = Text::new("Eigene", "Own");
pub const DELETE_GROUP: Text = Text::new("Gruppe löschen", "Delete group");
/// In the group strip's context menu; the dialog asks before it deletes.
pub const DELETE_GROUP_MENU: Text = Text::new("Gruppe löschen…", "Delete group…");
pub const NEW_GROUP_NAME: Text = Text::new("Gruppe", "Group");
pub const GROUP: Text = Text::new("Gruppe", "Group");
pub const NO_GROUP: Text = Text::new("keine", "none");
pub const LINKED: Text = Text::new("Gekoppelt", "Linked");
pub const COPY_TO_GROUP: Text = Text::new("Für Gruppe übernehmen", "Apply to group");
pub const MASTER: Text = Text::new("Master", "Master");
pub const MASTER_LEVEL: Text = Text::new("Master-Pegel", "Master level");
pub const MUTE_ALL: Text = Text::new("Alle stumm", "Mute all");
pub const HISTORY: Text = Text::new("Verlauf", "History");
pub const HISTORY_TITLE: Text = Text::new("Verlauf dieser Sitzung", "History of this session");
pub const NO_HISTORY: Text = Text::new(
    "Noch keine Änderungen in dieser Sitzung.",
    "No changes in this session yet.",
);
/// Takes a write back, in the history and the EQ window alike.
pub const UNDO: Text = Text::new("Rückgängig", "Undo");
/// A history entry the speaker refused or never got.
pub const NOT_WRITTEN: Text = Text::new("Nicht geschrieben", "Not written");
pub const TAKEN_BACK: Text = Text::new("Zurückgenommen", "Undone");
pub const BEFORE_UNKNOWN: Text = Text::new(
    "Der Wert vorher ist unbekannt",
    "The value before is unknown",
);
pub const CHANGED_SINCE: Text = Text::new("Seitdem geändert", "Changed since");
pub const REVERT_ANYWAY: Text = Text::new("Trotzdem rückgängig machen?", "Undo anyway?");
pub const REVERT: Text = Text::new("Trotzdem rückgängig", "Undo anyway");
pub const SWITCH_ON: Text = Text::new("an", "on");
pub const SWITCH_OFF: Text = Text::new("aus", "off");
pub const EQ: Text = Text::new("EQ", "EQ");
pub const EQ_SUM: Text = Text::new("Summe", "Sum");
pub const EQ_COMPUTED: Text = Text::new(
    "Berechnet aus den Einstellungen, nicht gemessen",
    "Computed from the settings, not measured",
);
pub const EQ_NOT_REPORTED: Text = Text::new(
    "Der Lautsprecher hat diesen EQ noch nicht gemeldet.",
    "The speaker has not reported this EQ yet.",
);
/// The KH 750's outputs, measured 2026-10-19: `out` drives its own woofer, `out1`/`out2` are the
/// sockets A and B.
pub const EQ_OUTPUT_SUBWOOFER: Text = Text::new("Subwoofer", "Subwoofer");
/// Shown for a value the speaker has not reported.
pub const UNKNOWN_VALUE: &str = "?";
pub const BAND: Text = Text::new("Band", "Band");
pub const EQ_RESET: Text = Text::new("Flach stellen", "Make flat");
pub const EQ_FULL_RESET: Text = Text::new("Zurücksetzen", "Reset");
pub const EQ_PRESET_SAVE: Text = Text::new("Als EQ-Preset speichern…", "Save as EQ preset…");
pub const EQ_PRESET_LOAD: Text = Text::new("EQ-Preset laden…", "Load EQ preset…");
pub const EQ_PRESET_SAVE_TITLE: Text = Text::new("EQ als Preset", "EQ as a preset");
pub const EQ_PRESET_LOAD_TITLE: Text = Text::new("EQ-Preset laden", "Load an EQ preset");
pub const EQ_IMPORT: Text = Text::new("Importieren…", "Import…");
pub const EQ_EXPORT: Text = Text::new("Exportieren…", "Export…");
pub const EQ_IMPORT_TITLE: Text = Text::new("EQ importieren", "Import an EQ");
pub const EQ_FILE_TOO_LARGE: Text = Text::new(
    "Die Datei ist zu groß für eine Filterdatei.",
    "The file is too large for a filter file.",
);
pub const SYSTEM_AUDIO_SHOW: Text = Text::new(
    "Windows-Lautstärke im Pult zeigen",
    "Show the Windows volume on the desk",
);
pub const SYSTEM_AUDIO_DEVICE: Text = Text::new("Gerät", "Device");
pub const SYSTEM_AUDIO_DEFAULT: Text = Text::new("Windows-Standardgerät", "Windows default device");
pub const SYSTEM_AUDIO_UNAVAILABLE: Text = Text::new("nicht verfügbar", "not available");
pub const SYSTEM_AUDIO_NAME: Text = Text::new("Windows", "Windows");
pub const SYSTEM_VOLUME: Text = Text::new("Windows-Lautstärke", "Windows volume");
pub const SYSTEM_METERS: Text = Text::new("Windows-Pegel", "Windows level");
pub const SYSTEM_MUTE: Text = Text::new("Windows stumm", "Windows mute");
pub const EQ_DISPLAY: Text = Text::new("EQ-Darstellung", "EQ display");
pub const BAND_CURVES_ALL: Text = Text::new("Einzelkurven aller Bänder", "Curves of all bands");
pub const BAND_CURVES_CHOSEN: Text = Text::new(
    "Einzelkurve nur für das gewählte Band und das unter der Maus",
    "Curve only for the chosen band and the one under the pointer",
);
pub const BAND_COLORS: Text = Text::new("Bandfarben", "Band colours");
pub const CURVE_COLORS: Text = Text::new("Kurvenfarben", "Curve colours");
pub const DEFAULT_COLORS: Text = Text::new("Standardfarben", "Default colours");
pub const COMPARE_WITH: Text = Text::new("Vergleichen mit:", "Compare with:");
pub const CROSSOVER_WARNING: Text = Text::new(
    "Das ist eine Weiche: ihr Tief- bzw. Hochpass entfällt, der Ausgang spielt ungefiltert.",
    "This is a crossover: its low or high pass goes, the output plays unfiltered.",
);
pub const PROTECT_CROSSOVERS: Text = Text::new(
    "Weichen schützen (kein Flachstellen, Zurücksetzen oder Importieren, keine EQ-Presets)",
    "Protect crossovers (no flattening, reset, import or EQ presets)",
);
pub const NO_TREND_VALUES: Text = Text::new("Noch keine Werte", "No values yet");
pub const TREND_NOW: Text = Text::new("jetzt", "now");
pub const WARNING_TREND: Text = Text::new("Warnungsverlauf", "Warning trend");
pub const NO_WARNING: Text = Text::new("keine Warnung", "no warning");
pub const NOT_READ: Text = Text::new("nicht erreichbar", "not reachable");
/// A trend's time before its first reading.
pub const NOT_RECORDED_YET: Text = Text::new("noch nicht aufgezeichnet", "not recorded yet");
/// A speaker of a history entry that has been removed since.
pub const REMOVED_SPEAKER: Text = Text::new("entfernter Lautsprecher", "removed speaker");
pub const CONTROL_HINT: Text = Text::new(
    "Shift: fein · Doppelklick: Wert eingeben",
    "Shift: fine · Double-click: type a value",
);
/// Faders take no typed value themselves (the strip's readout below does).
pub const FADER_HINT: Text = Text::new("Shift: fein", "Shift: fine");
pub const PANEL_TITLE: Text = Text::new("Lautsprecher", "Speaker");
/// The button under a strip that opens the speaker's panel.
pub const DETAILS: Text = Text::new("Details", "Details");
pub const SETTINGS: Text = Text::new("Einstellungen", "Settings");
pub const COMPACT: Text = Text::new("Kompakt", "Compact");
pub const COMPACT_HINT: Text = Text::new("Strg+K", "Ctrl+K");
pub const CLOSE_SETTINGS: Text = Text::new("Einstellungen schließen", "Close settings");
pub const SETTINGS_TITLE: Text = Text::new("Einstellungen", "Settings");
pub const THEME: Text = Text::new("Darstellung", "Appearance");
/// Theme and language that follow Windows.
pub const LIKE_WINDOWS: Text = Text::new("wie Windows", "like Windows");
pub const THEME_LIGHT: Text = Text::new("Hell", "Light");
pub const THEME_DARK: Text = Text::new("Dunkel", "Dark");
pub const LANGUAGE_LABEL: Text = Text::new("Sprache", "Language");
/// Each language is named in itself, so it can be found from either.
pub const LANGUAGE_GERMAN: &str = "Deutsch";
pub const LANGUAGE_ENGLISH: &str = "English";
pub const METER_RANGE: Text = Text::new("Pegelanzeige ab", "Meters from");
pub const SHOW_RESPONSE_TIMES: Text = Text::new("Antwortzeiten anzeigen", "Show response times");
pub const LAYOUTS_PER_MODEL: Text = Text::new(
    "Anordnung gleich für alle Lautsprecher eines Modells",
    "Same layout for every speaker of a model",
);
pub const SHOW_MASTER: Text = Text::new("Master-Zug anzeigen", "Show master strip");
pub const DELAY_SHOWN_IN: Text = Text::new("Verzögerung anzeigen in", "Show delays in");
pub const DELAY_IN_MILLISECONDS: Text = Text::new("Millisekunden", "milliseconds");
/// 343 m/s, the speed of sound at 20 °C, as `catalog` converts with.
pub const DELAY_IN_METERS: Text = Text::new(
    "Metern (Schallweg bei 20 °C)",
    "meters (sound path at 20 °C)",
);
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
pub const SAVE_PRESET: Text = Text::new("Als Preset speichern…", "Save as preset…");
pub const LOAD_PRESET: Text = Text::new("Preset laden…", "Load preset…");
pub const SAVE_PRESET_TITLE: Text = Text::new("Einstellungen als Preset", "Settings as a preset");
pub const LOAD_PRESET_TITLE: Text = Text::new("Preset laden", "Load a preset");
pub const PRESET_NAME: Text = Text::new("Name des Presets", "Preset name");
pub const SAVE: Text = Text::new("Speichern", "Save");
pub const WRITE_PRESET: Text = Text::new("Schreiben", "Write");
pub const DELETE_PRESET: Text = Text::new("Löschen", "Delete");
pub const REPLACE_PRESET: Text = Text::new("Ersetzen", "Replace");
pub const PRESET_REPLACES: Text = Text::new(
    "Ein Preset dieses Namens wird ersetzt.",
    "A preset of this name is replaced.",
);
pub const NO_PRESETS: Text = Text::new("Noch keine Presets.", "No presets yet.");
pub const PRESET_UNCHANGED: Text = Text::new(
    "Der Lautsprecher hat schon die Werte des Presets.",
    "The speaker already has the preset's values.",
);
pub const IMPORT_UNCHANGED: Text = Text::new(
    "Der Lautsprecher hat schon die Werte der Datei.",
    "The speaker already has the file's values.",
);
pub const FULL_SCAN: Text = Text::new("Komplett scannen", "Full scan");
pub const FULL_SCAN_RUNNING: Text = Text::new("Scan läuft …", "Scanning …");
pub const APPLY: Text = Text::new("Übernehmen", "Apply");
pub const ON: Text = Text::new("an", "on");
pub const OFF: Text = Text::new("aus", "off");
pub const CELSIUS: &str = "°C";
pub const MILLISECONDS: &str = "ms";
pub const METERS: &str = "m";

pub const ALL_VALUES_FILTER_LABEL: Text = Text::new("Pfad enthält", "Path contains");
pub const ALL_VALUES_RELOAD: Text = Text::new("Neu einlesen", "Read again");
pub const ALL_VALUES_LOADING: Text = Text::new("Lese alle Werte …", "Reading all values …");
pub const ALL_VALUES_FAILED: Text = Text::new(
    "Die Werte konnten nicht gelesen werden. Das Gerät ist nicht erreichbar oder hat die Abfrage abgelehnt.",
    "The values could not be read. The device is unreachable or refused the request.",
);
/// A value the speaker refused to give; its own words stay in the copied line.
pub const LEAF_REFUSED: Text = Text::new("vom Gerät abgelehnt", "refused by the device");
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
pub const SCAN_AGAIN: Text = Text::new("Erneut suchen", "Scan again");
pub const ALREADY_ADDED: Text = Text::new("bereits hinzugefügt", "already added");

pub const REMOVE: Text = Text::new("Entfernen", "Remove");
pub const REMOVE_TITLE: Text = Text::new("Lautsprecher entfernen?", "Remove speaker?");

pub const UNREACHABLE: Text = Text::new(
    "nicht erreichbar (aus oder Standby)",
    "unreachable (off or standby)",
);
pub const WRITE_FAILED: Text = Text::new("Schreiben fehlgeschlagen", "Write failed");

/// The edit mode's note that a strip layout belongs to the model, not to the one speaker.
pub fn applies_to_every(model: &str) -> String {
    pick(
        format!("gilt für alle {model}"),
        format!("applies to every {model}"),
    )
}

pub fn version(version: &str) -> String {
    format!("Version {version}")
}

pub fn update_available(tag: &str) -> String {
    pick(
        format!("Neue Version {tag} verfügbar"),
        format!("Version {tag} is available"),
    )
}

/// What the start says about loading the settings; nothing when they were read in full.
pub fn settings_notice(outcome: LoadOutcome) -> Option<Text> {
    match outcome {
        LoadOutcome::Read => None,
        LoadOutcome::PartlyUnderstood => Some(SETTINGS_PARTLY_UNDERSTOOD),
        LoadOutcome::SetAside => Some(SETTINGS_SET_ASIDE),
        LoadOutcome::Untouchable => Some(SETTINGS_UNTOUCHABLE),
    }
}

/// Hover text of a speaker that checking would move out of `group`.
pub fn in_other_group(group: &str) -> String {
    pick(format!("Jetzt in „{group}“"), format!("Now in \"{group}\""))
}

/// Asked before a preset or a group is deleted.
pub fn delete_question(name: &str) -> String {
    pick(format!("„{name}“ löschen?"), format!("Delete \"{name}\"?"))
}

/// The EQ sum of one input, where an output has a crossover per input.
pub fn eq_input_sum(input: &str) -> String {
    pick(format!("Summe {input}"), format!("Sum {input}"))
}

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
        Some(db) if db < 0 => format!("−{}", db.unsigned_abs()),
        Some(db) => db.to_string(),
        None => "–".to_owned(),
    }
}

/// The panel's knob beside a number field, named apart from the field.
pub fn knob_label(label: &str) -> String {
    pick(format!("{label} (Regler)"), format!("{label} (knob)"))
}

/// A mark on the meter scale, without unit to fit beside the meters.
/// With the real minus sign, as the number under each meter has.
pub fn scale_mark(db: f64) -> String {
    match db < 0.0 {
        true => format!("−{}", -db),
        false => format!("{db}"),
    }
}

pub fn meter_floor(db: f64) -> String {
    format!("{} dB", scale_mark(db))
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

/// Names the group strip's "Details" apart from the speakers' ones.
pub fn group_details(group: &str) -> String {
    pick(
        format!("Details der Gruppe {group}"),
        format!("Details of group {group}"),
    )
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
        Refusal::OutOfRange { min, max, unit } => {
            let (min, max) = (number(*min), number(*max));
            let range = format!("{min} {unit}").trim_end().to_owned();
            let range_end = format!("{max} {unit}").trim_end().to_owned();
            pick(
                format!("Wert außerhalb von {range} bis {range_end}."),
                format!("Value outside {range} to {range_end}."),
            )
        }
        Refusal::Invalid(error) => describe_value_error(error),
    }
}

pub fn describe_rejection(code: i64) -> String {
    match code {
        DeviceError::LOCAL_MODE => Text::new(
            "Gerät steht auf LOCAL (Schalter hinten), Schreiben gesperrt",
            "Device is set to LOCAL (rear switch), writing locked",
        )
        .into(),
        DeviceError::READ_ONLY => {
            Text::new("Gerät lehnt ab: nur lesbar", "Device refuses: read only").into()
        }
        DeviceError::NOT_ACCEPTABLE => Text::new(
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

/// The name of an EQ container's output: the subwoofer, or output A or B as the panel names them.
pub fn eq_output(group: Group) -> &'static str {
    match group {
        Group::Output1 | Group::Output2 => group.title(language()),
        _ => EQ_OUTPUT_SUBWOOFER.get(),
    }
}

/// An EQ named with its output, where the model has several (`EQ 2` is on each of them).
pub fn eq_on_output(output: &str, container: &str) -> String {
    format!("{output}, {container}")
}

/// A value in the history, with its unit.
pub fn history_value(value: &Value, unit: Option<&str>) -> String {
    let text = match value {
        Value::Bool(true) => SWITCH_ON.get().to_owned(),
        Value::Bool(false) => SWITCH_OFF.get().to_owned(),
        Value::Number(number) => number
            .as_f64()
            .map_or_else(|| number.to_string(), self::number),
        Value::String(text) => text.clone(),
        Value::Array(entries) => entries
            .iter()
            .map(|entry| history_value(entry, None))
            .collect::<Vec<_>>()
            .join(" / "),
        other => other.to_string(),
    };
    match unit {
        Some(unit) => format!("{text} {unit}"),
        None => text,
    }
}

/// One change of a history entry.
pub fn history_change(speaker: &str, setting: &str, before: &str, after: &str) -> String {
    format!("{speaker} · {setting}: {before} → {after}")
}

/// An entry with several changes: the setting when all share it.
pub fn history_changes(setting: Option<&str>, count: usize) -> String {
    match setting {
        Some(setting) => pick(
            format!("{setting}: {count} Lautsprecher"),
            format!("{setting}: {count} speakers"),
        ),
        None => pick(format!("{count} Änderungen"), format!("{count} changes")),
    }
}

/// An entry that took back an earlier one.
pub fn history_take_back(time: &str, text: &str) -> String {
    pick(
        format!("Rücknahme von {time}: {text}"),
        format!("Undo of {time}: {text}"),
    )
}

pub fn clock_time(hour: u8, minute: u8, second: u8) -> String {
    format!("{hour:02}:{minute:02}:{second:02}")
}

/// A value that changed since it was written, in the question before taking it back.
pub fn changed_now(speaker: &str, setting: &str, now: &str, written: &str) -> String {
    pick(
        format!("{speaker} · {setting}: jetzt {now} statt {written}"),
        format!("{speaker} · {setting}: now {now} instead of {written}"),
    )
}

/// What the last take-back left out.
pub fn take_back_skipped(names: &[String]) -> String {
    let names = names.join(", ");
    pick(
        format!("Nicht zurückgenommen: {names}"),
        format!("Not undone: {names}"),
    )
}

/// Names a band's point in the EQ graph.
pub fn eq_band(number: usize) -> String {
    format!("{} {number}", BAND.get())
}

/// The panel window's title: "Lautsprecher · <name>".
pub fn panel_window_title(speaker: &str) -> String {
    format!("{} · {speaker}", PANEL_TITLE.get())
}

pub fn eq_window_title(speaker: &str) -> String {
    format!("EQ · {speaker}")
}

/// A frequency mark of the EQ graph: "50", "1k", "20k".
pub fn frequency_mark(hertz: f64) -> String {
    if hertz >= 1000.0 {
        return format!("{}k", hertz / 1000.0);
    }
    format!("{hertz}")
}

/// What a band change in the history touched: "EQ 2 Band 3 · Verstärkung".
pub fn eq_change_setting(container: &str, band: usize, leaf: &str) -> String {
    format!("{container} {} {band} · {leaf}", BAND.get())
}

/// A setting in the save dialog: "Pegel: 85 dB SPL".
pub fn preset_row(setting: &str, value: &str) -> String {
    format!("{setting}: {value}")
}

/// A preset in the load dialog.
pub fn preset_entry(name: &str, product: &str) -> String {
    format!("{name} ({product})")
}

/// One write of a loaded preset: "Pegel: 85 dB SPL → 90 dB SPL".
pub fn preset_change(setting: &str, current: &str, preset: &str) -> String {
    format!("{setting}: {current} → {preset}")
}

pub fn preset_skipped(paths: &[String]) -> String {
    pick(
        format!("Übersprungen, hier nicht schreibbar: {}", paths.join(", ")),
        format!("Skipped, not writable here: {}", paths.join(", ")),
    )
}

/// Where the full scan's report went.
pub fn scan_written(path: &str) -> String {
    pick(
        format!("Bericht geschrieben: {path}"),
        format!("Report written: {path}"),
    )
}

pub fn scan_failed(failure: ScanFailure) -> &'static str {
    let text = match failure {
        ScanFailure::Reading => Text::new(
            "Scan fehlgeschlagen: Der Lautsprecher hat nicht vollständig geantwortet.",
            "Scan failed: the speaker did not answer completely.",
        ),
        ScanFailure::Writing => Text::new(
            "Scan fehlgeschlagen: Der Bericht ließ sich neben der Exe nicht speichern.",
            "Scan failed: the report could not be saved next to the executable.",
        ),
    };
    text.get()
}

/// The device panel's trend heading: "Verlauf (2 h)".
pub fn trends_title(window_seconds: f64) -> String {
    let hours = hours(window_seconds);
    pick(format!("Verlauf ({hours} h)"), format!("Trend ({hours} h)"))
}

/// The left end of a trend's time axis: "−2 h" for whole hours, else "−25 min".
pub fn trend_start(span_seconds: f64) -> String {
    let minutes = whole_minutes(span_seconds);
    if minutes > 0 && minutes.is_multiple_of(MINUTES_PER_HOUR) {
        return format!("−{} h", minutes / MINUTES_PER_HOUR);
    }
    format!("−{minutes} min")
}

const MINUTES_PER_HOUR: u64 = 60;

const SECONDS_PER_HOUR: f64 = 3600.0;

fn hours(seconds: f64) -> f64 {
    seconds / SECONDS_PER_HOUR
}

/// Names the temperature line for screen readers, with the current value.
pub fn temperature_trend(current: &str) -> String {
    pick(
        format!("Temperaturverlauf, jetzt {current}"),
        format!("Temperature trend, now {current}"),
    )
}

pub fn trend_range(low: &str, high: &str) -> String {
    format!("{low} … {high}")
}

/// What a trend held at the hovered time.
pub fn trend_hover(minutes_ago: u64, value: &str) -> String {
    pick(
        format!("vor {minutes_ago} min: {value}"),
        format!("{minutes_ago} min ago: {value}"),
    )
}

/// The Windows volume under its fader: "52 %".
pub fn percent(value: f64) -> String {
    format!("{} %", number_text(value.round().to_string()))
}

/// An EQ preset in the load dialog: its name and band count.
pub fn eq_preset_entry(name: &str, bands: usize) -> String {
    pick(
        format!("{name} ({bands} Bänder)"),
        format!("{name} ({bands} bands)"),
    )
}

/// A band an EQ preset would change, in the load dialog's preview.
pub fn eq_preset_band(number: usize) -> String {
    pick(
        format!("Band {number} ändert sich"),
        format!("Band {number} changes"),
    )
}

/// The legend entry of another speaker's curve in the EQ graph.
pub fn compared_curve(speaker: &str) -> String {
    pick(format!("{speaker} (Summe)"), format!("{speaker} (sum)"))
}

/// Asked before a full reset, in the EQ window.
pub fn eq_full_reset_question(container: &str, kind: &str, frequency: f64, q: f64) -> String {
    let (frequency, q) = (number(frequency), number(q));
    pick(
        format!(
            "Alle Bänder von {container} zurücksetzen? Aus, {kind}, {frequency} Hz, Q {q}, Pegel und Anhebung 0 dB."
        ),
        format!(
            "Reset every band of {container}? Off, {kind}, {frequency} Hz, Q {q}, level and boost 0 dB."
        ),
    )
}

/// Asked before a reset to flat, in the EQ window.
pub fn eq_reset_question(container: &str) -> String {
    pick(
        format!("Alle Bänder von {container} flach stellen? Pegel und Anhebung auf 0 dB."),
        format!("Make every band of {container} flat? Level and boost to 0 dB."),
    )
}

/// A refused write of one setting or EQ leaf, with the setting named.
pub fn setting_refusal(leaf: &str, reason: &str) -> String {
    format!("{leaf}: {reason}")
}

/// What a write of every band touched: "EQ 2 · Pegel, alle Bänder".
pub fn eq_all_bands_setting(container: &str, leaf: &str) -> String {
    pick(
        format!("{container} · {leaf}, alle Bänder"),
        format!("{container} · {leaf}, all bands"),
    )
}

/// A refused band write, under the EQ graph.
pub fn eq_refusal(band: usize, leaf: &str, reason: &str) -> String {
    format!("{} {band} · {leaf}: {reason}", BAND.get())
}

/// The import preview's line naming the file.
pub fn eq_import_from(file: &str) -> String {
    pick(format!("Aus {file}"), format!("From {file}"))
}

/// The sound commands of the file that the import does not take over.
pub fn eq_import_ignored(commands: &[String]) -> String {
    let list = commands.join(", ");
    pick(
        format!("Nicht übernommen: {list}"),
        format!("Not taken over: {list}"),
    )
}

pub fn eq_file_unreadable(file: &str, reason: &str) -> String {
    pick(
        format!("{file} lässt sich nicht lesen: {reason}"),
        format!("{file} cannot be read: {reason}"),
    )
}

pub fn eq_file_unwritable(file: &str, reason: &str) -> Vec<String> {
    vec![pick(
        format!("{file} lässt sich nicht schreiben: {reason}"),
        format!("{file} cannot be written: {reason}"),
    )]
}

/// Why a filter file cannot be imported.
pub fn eq_import_error(error: &ImportError) -> String {
    match error {
        ImportError::Line { line, problem } => pick(
            format!("Zeile {line}: {}", line_problem(problem)),
            format!("Line {line}: {}", line_problem(problem)),
        ),
        ImportError::NoFilters => Text::new(
            "Die Datei enthält keine Filterzeilen.",
            "The file holds no filter lines.",
        )
        .get()
        .to_owned(),
        ImportError::TooManyBands { band, bands } => pick(
            format!("Die Datei nennt Band {band}, dieser EQ hat {bands} Bänder."),
            format!("The file names band {band}, this EQ has {bands} bands."),
        ),
        ImportError::SeveralChannels => Text::new(
            "Die Datei hat Filter für mehrere Kanäle oder unter Bedingungen; ein EQ nimmt die eines Kanals.",
            "The file has filters for several channels or under conditions; an EQ takes one channel's.",
        )
        .get()
        .to_owned(),
    }
}

fn line_problem(problem: &LineProblem) -> String {
    match problem {
        LineProblem::UnknownType(kind) => pick(
            format!("Filtertyp „{kind}“ gibt es am Lautsprecher nicht."),
            format!("The speaker has no filter type \"{kind}\"."),
        ),
        LineProblem::UnsupportedForm(form) => pick(
            format!(
                "Kuhschwanzfilter mit Steilheit in dB oder Eckfrequenz („{form}“) werden nicht übernommen."
            ),
            format!("Shelving filters by slope in dB or corner frequency (\"{form}\") are not taken over."),
        ),
        LineProblem::Missing(parameter) => pick(
            format!("{} fehlt.", parameter_name(*parameter)),
            format!("{} is missing.", parameter_name(*parameter)),
        ),
        LineProblem::BadNumber(token) => pick(
            format!("„{token}“ ist keine Zahl."),
            format!("\"{token}\" is not a number."),
        ),
        LineProblem::UnknownState(state) => pick(
            format!("„{state}“ statt ON oder OFF."),
            format!("\"{state}\" instead of ON or OFF."),
        ),
        LineProblem::SameBand(band) => pick(
            format!("Band {band} steht schon in einer früheren Zeile."),
            format!("Band {band} is already in an earlier line."),
        ),
        LineProblem::BadQ => Text::new(
            "Q bzw. Bandbreite ergibt kein Q über 0.",
            "Q or bandwidth gives no Q above 0.",
        )
        .get()
        .to_owned(),
    }
}

fn parameter_name(parameter: Parameter) -> &'static str {
    match parameter {
        Parameter::Frequency => "Fc",
        Parameter::Gain => "Gain",
        Parameter::Q => "Q",
    }
}

/// After an export, one line each: where it went, then what it could not carry.
pub fn eq_export_report(file: &str, notes: &[ExportNote]) -> Vec<String> {
    let mut lines = vec![pick(
        format!("Gespeichert: {file}"),
        format!("Saved: {file}"),
    )];
    lines.extend(notes.iter().map(export_note));
    lines
}

fn export_note(note: &ExportNote) -> String {
    match note {
        ExportNote::Off(band) => pick(
            format!("Band {band} ist aus und steht nur als Kommentar in der Datei."),
            format!("Band {band} is off and only a comment in the file."),
        ),
        ExportNote::FlatLevel { band, gain } => pick(
            format!(
                "Band {band}: Pegel {} dB nicht exportiert (APO kennt ihn nicht).",
                one_decimal(*gain)
            ),
            format!(
                "Band {band}: level {} dB not exported (APO has none).",
                one_decimal(*gain)
            ),
        ),
        ExportNote::NotExportable { band, kind } => pick(
            format!("Band {band}: {kind} nicht exportiert."),
            format!("Band {band}: {kind} not exported."),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Scale, floor setting and the number under the meter show one and the same minus.
    #[test]
    fn meter_numbers_share_the_minus_sign() {
        assert_eq!(scale_mark(-6.0), "−6");
        assert_eq!(scale_mark(0.0), "0");
        assert_eq!(meter_floor(-60.0), "−60 dB");
        assert_eq!(meter_readout(Some(-18)), "−18");
    }

    /// The span is a difference of clock readings, never exactly 3600: whole hours are told
    /// by the minutes it shows.
    #[test]
    fn a_trend_of_an_hour_starts_at_one_hour() {
        assert_eq!(trend_start(3600.4), "−1 h");
        assert_eq!(trend_start(7199.9), "−2 h");
        assert_eq!(trend_start(1500.0), "−25 min");
        assert_eq!(trend_start(60.0), "−1 min");
    }

    #[test]
    fn a_list_value_reads_entry_by_entry() {
        let value = serde_json::json!([0.0, -20.5]);
        assert_eq!(
            history_value(&value, Some("dB")).replace(',', "."),
            "0 / -20.5 dB"
        );
    }

    #[test]
    fn a_value_that_shows_as_zero_has_no_minus() {
        let plain = |text: String| text.replace(',', ".");
        assert_eq!(number(-0.0), "0");
        assert_eq!(plain(one_decimal(-0.04)), "0.0");
        assert_eq!(plain(two_decimals(-0.001)), "0.00");
        assert_eq!(percent(-0.2), "0 %");
        assert_eq!(plain(one_decimal(-0.06)), "-0.1");
        assert_eq!(number(-2.5).replace(',', "."), "-2.5");
    }

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
