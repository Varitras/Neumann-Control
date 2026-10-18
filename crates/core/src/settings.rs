//! Persistent settings stored next to the executable.

use std::cell::Cell;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};

pub const SETTINGS_FILE: &str = "settings.json";
pub const LOG_FILE: &str = "neumann-control.log";

const TEMPORARY_SUFFIX: &str = ".tmp";
const QUARANTINE_SUFFIX: &str = ".bad";

thread_local! {
    /// How many values the current `load` replaced or left out; then the file is copied aside
    /// before the next save drops them for good.
    static NOT_UNDERSTOOD: Cell<usize> = const { Cell::new(0) };
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Every remembered speaker, added by address or chosen from a scan. The key keeps its
    /// first name so that files written before scans were remembered load unchanged.
    #[serde(deserialize_with = "readable_items")]
    pub manual: Vec<ManualSpeaker>,
    /// The window as the desk had it, and as the compact mode had it.
    #[serde(deserialize_with = "or_default")]
    pub window: Option<WindowGeometry>,
    #[serde(deserialize_with = "or_default")]
    pub compact_window: Option<WindowGeometry>,
    pub desk: DeskSettings,
    #[serde(deserialize_with = "readable_items")]
    pub groups: Vec<GroupSettings>,
    /// Speaker identities (`serial:<serial>` or `address:<address>`) in the order of the desk.
    #[serde(deserialize_with = "or_default")]
    pub order: Vec<String>,
}

impl Settings {
    /// The window as the current mode, desk or compact, last had it.
    pub fn window_of_mode(&self) -> Option<WindowGeometry> {
        if self.desk.compact {
            self.compact_window
        } else {
            self.window
        }
    }

    /// Keeps `geometry` for the current mode.
    pub fn remember_window(&mut self, geometry: WindowGeometry) {
        if self.desk.compact {
            self.compact_window = Some(geometry);
        } else {
            self.window = Some(geometry);
        }
    }

    /// Leaves the current mode, keeping its window `current`, and returns the window the other
    /// mode last had.
    pub fn switch_mode(&mut self, current: Option<WindowGeometry>) -> Option<WindowGeometry> {
        if let Some(current) = current {
            self.remember_window(current);
        }
        self.desk.compact = !self.desk.compact;
        self.window_of_mode()
    }
}

/// A group of speakers with a group strip; a speaker is in at most one group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupSettings {
    pub name: String,
    /// Index into the app's group colour palette.
    pub color: u8,
    /// Speaker identities, as in `Settings::order`.
    pub members: Vec<String>,
    /// Settings a change on one member sends to all members.
    #[serde(default)]
    pub linked: Vec<Linkable>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Linkable {
    Level,
    Mute,
    Standby,
    Input,
}

/// What the settings dialog changes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeskSettings {
    #[serde(deserialize_with = "or_default")]
    pub theme: ThemeChoice,
    #[serde(deserialize_with = "or_default")]
    pub meter_floor: MeterFloor,
    #[serde(deserialize_with = "or_default")]
    pub show_response_times: bool,
    #[serde(deserialize_with = "or_default")]
    pub language: Language,
    #[serde(deserialize_with = "or_default")]
    pub compact: bool,
}

impl Default for DeskSettings {
    fn default() -> DeskSettings {
        DeskSettings {
            theme: ThemeChoice::System,
            meter_floor: MeterFloor::Minus120,
            show_response_times: true,
            language: Language::German,
            compact: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ThemeChoice {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Language {
    #[default]
    German,
    English,
}

impl Language {
    pub const ALL: [Language; 2] = [Language::German, Language::English];
}

/// The lowest level a meter shows; quiet room volume reads about -90 dB on the inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MeterFloor {
    #[default]
    Minus120,
    Minus90,
    Minus60,
}

impl MeterFloor {
    pub const ALL: [MeterFloor; 3] = [
        MeterFloor::Minus120,
        MeterFloor::Minus90,
        MeterFloor::Minus60,
    ];

    pub const fn db(self) -> f64 {
        match self {
            MeterFloor::Minus120 => -120.0,
            MeterFloor::Minus90 => -90.0,
            MeterFloor::Minus60 => -60.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManualSpeaker {
    pub address: String,
    pub port: u16,
    #[serde(default)]
    pub serial: Option<String>,
}

/// Outer position and inner size of the main window in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowGeometry {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// A value this version cannot read (one a later version wrote, a hand edit) takes its default
/// instead of costing the whole file, and so the remembered speakers.
fn or_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned + Default,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(T::deserialize(value).unwrap_or_else(|error| {
        log::warn!(
            "a settings value is not understood ({:?}); using its default",
            error.classify()
        );
        NOT_UNDERSTOOD.with(|count| count.set(count.get() + 1));
        T::default()
    }))
}

/// A list keeps the entries this version can read; one it cannot is left out, not the list.
/// Something that is not a list at all means a damaged file, which `load` sets aside whole.
fn readable_items<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let items = Vec::<serde_json::Value>::deserialize(deserializer)?;
    let total = items.len();
    let readable: Vec<T> = items
        .into_iter()
        .filter_map(|item| T::deserialize(item).ok())
        .collect();
    let left_out = total - readable.len();
    if left_out > 0 {
        log::warn!("{left_out} settings entries are not understood; left out");
        NOT_UNDERSTOOD.with(|count| count.set(count.get() + left_out));
    }
    Ok(readable)
}

/// Loads the settings; a missing file yields the defaults, an unreadable or
/// malformed one is moved aside (never overwriting an earlier one) and also
/// yields the defaults. Within a readable file, values this version does not understand take
/// their defaults (see `or_default`).
pub fn load(path: &Path) -> Settings {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => return Settings::default(),
        Err(error) => {
            log::warn!(
                "settings file could not be read ({}); using defaults",
                error.kind()
            );
            quarantine(path);
            return Settings::default();
        }
    };
    NOT_UNDERSTOOD.with(|count| count.set(0));
    match serde_json::from_slice(&bytes) {
        Ok(settings) => {
            if NOT_UNDERSTOOD.with(Cell::get) > 0 {
                keep_a_copy(path);
            }
            settings
        }
        Err(error) => {
            // Only the error category is logged: the message can quote file contents,
            // which include speaker addresses.
            log::warn!(
                "settings file is malformed ({:?}); using defaults",
                error.classify()
            );
            quarantine(path);
            Settings::default()
        }
    }
}

/// Writes through a temporary file in the same directory so a crash never
/// leaves a truncated settings file behind.
pub fn save(settings: &Settings, path: &Path) -> io::Result<()> {
    let temporary = with_suffix(path, TEMPORARY_SUFFIX);
    let serialized = serde_json::to_vec_pretty(settings)?;
    let mut file = File::create(&temporary)?;
    file.write_all(&serialized)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, path)
}

/// Copies the file aside, as `quarantine` would move it, keeping the original in place.
fn keep_a_copy(path: &Path) {
    let target = free_quarantine_path(path);
    if let Err(error) = fs::copy(path, &target) {
        log::warn!("settings file could not be copied aside ({})", error.kind());
    }
}

fn quarantine(path: &Path) {
    let target = free_quarantine_path(path);
    if let Err(error) = fs::rename(path, &target) {
        log::warn!("settings file could not be moved aside ({})", error.kind());
    }
}

fn free_quarantine_path(path: &Path) -> PathBuf {
    let first = with_suffix(path, QUARANTINE_SUFFIX);
    if !first.exists() {
        return first;
    }
    (1..)
        .map(|number| with_suffix(&first, &format!(".{number}")))
        .find(|candidate| !candidate.exists())
        .expect("an unbounded range always yields a free name")
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name: OsString = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}
