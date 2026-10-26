//! Persistent settings stored next to the executable.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};

use crate::ssc::wire::SscPath;

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
    /// The windows of their own (device panel, EQ, all values) as the user left them, by kind
    /// and speaker, in screen pixels like `window`.
    #[serde(
        deserialize_with = "readable_entries",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub windows: BTreeMap<String, WindowGeometry>,
    #[serde(deserialize_with = "or_default")]
    pub desk: DeskSettings,
    #[serde(deserialize_with = "readable_items")]
    pub groups: Vec<GroupSettings>,
    /// Speaker identities (`serial:<serial>` or `address:<address>`) in the order of the desk.
    #[serde(deserialize_with = "or_default")]
    pub order: Vec<String>,
    #[serde(deserialize_with = "readable_items")]
    pub presets: Vec<SettingsPreset>,
    #[serde(deserialize_with = "readable_items")]
    pub eq_presets: Vec<EqPreset>,
    #[serde(deserialize_with = "or_default")]
    pub system_audio: SystemAudioSettings,
    #[serde(deserialize_with = "or_default")]
    pub eq_display: EqDisplaySettings,
    /// Speaker identity → its strip's colour; a speaker not listed takes its group's.
    #[serde(
        deserialize_with = "readable_entries",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub strip_colors: BTreeMap<String, ColorChoice>,
    /// Serial → what that speaker last reported about itself, so it keeps its name while off.
    #[serde(
        deserialize_with = "readable_entries",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub known: BTreeMap<String, KnownSpeaker>,
}

/// A speaker's own name and product as it reported them when it last connected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownSpeaker {
    pub name: String,
    pub product: String,
}

/// How the EQ graph draws its bands, and the colours the user chose instead of the defaults.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct EqDisplaySettings {
    #[serde(deserialize_with = "or_default")]
    pub band_curves: BandCurves,
    /// Band number (from 1) → `#rrggbb`; a band not listed has its default colour.
    #[serde(deserialize_with = "readable_entries")]
    pub band_colors: BTreeMap<String, String>,
    /// Container below its output (`eq2`, `eq1/in1`, ...) → `#rrggbb`.
    #[serde(deserialize_with = "readable_entries")]
    pub curve_colors: BTreeMap<String, String>,
}

/// Which bands show their own curve in the EQ graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BandCurves {
    #[default]
    All,
    /// The chosen band and the one under the pointer.
    ChosenAndHovered,
}

/// The strip with Windows' own volume of a playback device.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SystemAudioSettings {
    pub shown: bool,
    /// The endpoint id Windows gives the device; `None`: Windows' default playback device.
    pub device: Option<String>,
}

/// Chosen settings of a speaker under a name, to write to it or another speaker later.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsPreset {
    pub name: String,
    /// The product it was saved from, shown with the name.
    pub product: String,
    /// SSC path → value.
    pub values: BTreeMap<String, serde_json::Value>,
}

/// One EQ container's bands under a name: its six leaves, each an array of `bands` entries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EqPreset {
    pub name: String,
    pub bands: usize,
    /// Leaf name (`gain`, `boost`, ...) → array.
    pub leaves: BTreeMap<String, serde_json::Value>,
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

/// The colour of a strip: its default (a group's palette colour; a speaker's group colour),
/// none, or one the user chose.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorChoice {
    #[default]
    Default,
    Off,
    /// `#rrggbb`.
    Own(String),
}

impl ColorChoice {
    pub fn is_default(&self) -> bool {
        *self == ColorChoice::Default
    }
}

/// Whether a group has its strip on the desk (and its row in the compact mode); without, its
/// members stand on their own there.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GroupStrip {
    #[default]
    Shown,
    Hidden,
}

impl GroupStrip {
    pub fn is_default(&self) -> bool {
        *self == GroupStrip::Shown
    }
}

/// A group of speakers with a group strip; a speaker is in at most one group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupSettings {
    pub name: String,
    /// Index into the app's group colour palette.
    #[serde(default, deserialize_with = "or_default")]
    pub color: u8,
    /// The palette's colour, another one, or none on the group's strips.
    #[serde(
        default,
        deserialize_with = "or_default",
        skip_serializing_if = "ColorChoice::is_default"
    )]
    pub color_choice: ColorChoice,
    /// Speaker identities, as in `Settings::order`.
    #[serde(default, deserialize_with = "readable_items")]
    pub members: Vec<String>,
    /// Settings a change on one member sends to all members.
    #[serde(default, deserialize_with = "readable_items")]
    pub linked: Vec<Linkable>,
    #[serde(
        default,
        deserialize_with = "or_default",
        skip_serializing_if = "GroupStrip::is_default"
    )]
    pub strip: GroupStrip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Linkable {
    Level,
    Mute,
    Standby,
    Input,
}

/// How the desk looks and behaves: the settings dialog changes most of it, the strip editor the
/// strip layouts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeskSettings {
    #[serde(deserialize_with = "or_default")]
    pub theme: ThemeChoice,
    #[serde(deserialize_with = "or_default")]
    pub meter_floor: MeterFloor,
    #[serde(deserialize_with = "or_default")]
    pub show_response_times: bool,
    /// The master strip at the end of the desk and the master row in the compact mode.
    #[serde(deserialize_with = "or_default")]
    pub show_master: bool,
    #[serde(deserialize_with = "or_default")]
    pub language: LanguageChoice,
    #[serde(deserialize_with = "or_default")]
    pub compact: bool,
    #[serde(deserialize_with = "or_default")]
    pub crossovers: CrossoverGuard,
    #[serde(deserialize_with = "or_default")]
    pub delay_unit: DelayUnit,
    /// Per reported product name; a model without an entry shows the default strip.
    #[serde(
        deserialize_with = "readable_layouts",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub strip_layouts: BTreeMap<String, StoredStripLayout>,
    /// The buttons of a model's compact rows, per reported product name; without an entry the
    /// default ones.
    #[serde(
        deserialize_with = "readable_compact_layouts",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub compact_layouts: BTreeMap<String, Vec<CompactButton>>,
    /// Each speaker keeps its own strip and compact layout instead of its model's.
    #[serde(deserialize_with = "or_default")]
    pub layouts_per_speaker: bool,
    /// The log also takes the debug lines, for troubleshooting.
    #[serde(deserialize_with = "or_default")]
    pub detailed_log: bool,
    /// The desk's shortcuts work from other programs too, with Ctrl+Shift+Alt.
    #[serde(deserialize_with = "or_default")]
    pub global_shortcuts: bool,
    /// Like `strip_layouts`, per speaker identity, used with `layouts_per_speaker`.
    #[serde(
        deserialize_with = "readable_layouts",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub speaker_strip_layouts: BTreeMap<String, StoredStripLayout>,
    /// Like `compact_layouts`, per speaker identity, used with `layouts_per_speaker`.
    #[serde(
        deserialize_with = "readable_compact_layouts",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub speaker_compact_layouts: BTreeMap<String, Vec<CompactButton>>,
}

/// One element a channel strip shows besides its fixed part: a catalog path, or the EQ button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StripElement {
    Path(SscPath),
    Eq,
}

const EQ_ELEMENT: &str = "eq";

impl Serialize for StripElement {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            StripElement::Path(path) => path.serialize(serializer),
            StripElement::Eq => serializer.serialize_str(EQ_ELEMENT),
        }
    }
}

impl<'de> Deserialize<'de> for StripElement {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<StripElement, D::Error> {
        let text = String::deserialize(deserializer)?;
        if text == EQ_ELEMENT {
            return Ok(StripElement::Eq);
        }
        SscPath::parse(&text)
            .map(StripElement::Path)
            .map_err(serde::de::Error::custom)
    }
}

/// One button of a compact row after its fixed part: a switch's catalog path, EQ or Details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompactButton {
    Switch(SscPath),
    Eq,
    Details,
}

const DETAILS_BUTTON: &str = "details";

impl Serialize for CompactButton {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            CompactButton::Switch(path) => path.serialize(serializer),
            CompactButton::Eq => serializer.serialize_str(EQ_ELEMENT),
            CompactButton::Details => serializer.serialize_str(DETAILS_BUTTON),
        }
    }
}

impl<'de> Deserialize<'de> for CompactButton {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<CompactButton, D::Error> {
        let text = String::deserialize(deserializer)?;
        match text.as_str() {
            EQ_ELEMENT => Ok(CompactButton::Eq),
            DETAILS_BUTTON => Ok(CompactButton::Details),
            _ => SscPath::parse(&text)
                .map(CompactButton::Switch)
                .map_err(serde::de::Error::custom),
        }
    }
}

/// A model's strip elements above and below the fader, in order, as stored.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct StoredStripLayout {
    #[serde(default, deserialize_with = "readable_items")]
    pub above: Vec<StripElement>,
    #[serde(default, deserialize_with = "readable_items")]
    pub below: Vec<StripElement>,
}

impl Default for DeskSettings {
    fn default() -> DeskSettings {
        DeskSettings {
            theme: ThemeChoice::System,
            meter_floor: MeterFloor::Minus120,
            show_response_times: true,
            show_master: false,
            language: LanguageChoice::System,
            compact: false,
            crossovers: CrossoverGuard::Open,
            delay_unit: DelayUnit::Milliseconds,
            strip_layouts: BTreeMap::new(),
            compact_layouts: BTreeMap::new(),
            layouts_per_speaker: false,
            detailed_log: false,
            global_shortcuts: false,
            speaker_strip_layouts: BTreeMap::new(),
            speaker_compact_layouts: BTreeMap::new(),
        }
    }
}

/// Whether a crossover may be flattened or overwritten by an EQ preset: that removes the split
/// between the outputs. A value that is not understood falls back to protected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CrossoverGuard {
    #[default]
    Protected,
    Open,
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

/// The language the settings ask for; "like Windows" takes the system's display language.
/// The names of the fixed choices are those `Language` was stored under before.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LanguageChoice {
    #[default]
    System,
    German,
    English,
}

impl LanguageChoice {
    pub fn resolve(self, system: Language) -> Language {
        match self {
            LanguageChoice::System => system,
            LanguageChoice::German => Language::German,
            LanguageChoice::English => Language::English,
        }
    }
}

/// How delays are shown and typed: in time, or as the distance sound travels in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DelayUnit {
    #[default]
    Milliseconds,
    Meters,
}

impl DelayUnit {
    pub const ALL: [DelayUnit; 2] = [DelayUnit::Milliseconds, DelayUnit::Meters];
}

/// The lowest level a meter shows, in 10 dB steps; quiet room volume reads about -90 dB on the
/// inputs. The names stay as files have them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MeterFloor {
    #[default]
    Minus120,
    Minus110,
    Minus100,
    Minus90,
    Minus80,
    Minus70,
    Minus60,
    Minus50,
    Minus40,
    Minus30,
}

impl MeterFloor {
    pub const ALL: [MeterFloor; 10] = [
        MeterFloor::Minus120,
        MeterFloor::Minus110,
        MeterFloor::Minus100,
        MeterFloor::Minus90,
        MeterFloor::Minus80,
        MeterFloor::Minus70,
        MeterFloor::Minus60,
        MeterFloor::Minus50,
        MeterFloor::Minus40,
        MeterFloor::Minus30,
    ];

    pub const fn db(self) -> f64 {
        match self {
            MeterFloor::Minus120 => -120.0,
            MeterFloor::Minus110 => -110.0,
            MeterFloor::Minus100 => -100.0,
            MeterFloor::Minus90 => -90.0,
            MeterFloor::Minus80 => -80.0,
            MeterFloor::Minus70 => -70.0,
            MeterFloor::Minus60 => -60.0,
            MeterFloor::Minus50 => -50.0,
            MeterFloor::Minus40 => -40.0,
            MeterFloor::Minus30 => -30.0,
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
    /// Maximized; position and size are then the normal window's, for when it is restored.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub maximized: bool,
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
fn readable_items<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    // A value that is not a list at all loses only this list, not the whole file.
    let serde_json::Value::Array(items) = serde_json::Value::deserialize(deserializer)? else {
        log::warn!("a settings list is not a list; left out");
        NOT_UNDERSTOOD.with(|count| count.set(count.get() + 1));
        return Ok(Vec::new());
    };
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

/// Like `readable_items` for a map by name (colours, window places, known speakers): an entry
/// this version cannot read is left out, not the map.
fn readable_entries<'de, D, T>(deserializer: D) -> Result<BTreeMap<String, T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    readable_map(deserializer, "settings entries")
}

/// Like `readable_items` for the layout map: an entry this version cannot read is left out.
fn readable_layouts<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<String, StoredStripLayout>, D::Error>
where
    D: Deserializer<'de>,
{
    readable_map(deserializer, "strip layouts")
}

/// Like `readable_layouts`; within a model's list an unreadable button is left out alone.
fn readable_compact_layouts<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<String, Vec<CompactButton>>, D::Error>
where
    D: Deserializer<'de>,
{
    // A model whose entry is not a list is left out, so it gets the default buttons.
    let lists: BTreeMap<String, Vec<serde_json::Value>> =
        readable_map(deserializer, "compact layouts")?;
    lists
        .into_iter()
        .map(|(model, list)| {
            let buttons = readable_items(serde_json::Value::Array(list));
            Ok((model, buttons.map_err(serde::de::Error::custom)?))
        })
        .collect()
}

fn readable_map<'de, D, T>(deserializer: D, what: &str) -> Result<BTreeMap<String, T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let serde_json::Value::Object(entries) = serde_json::Value::deserialize(deserializer)? else {
        log::warn!("the {what} are not a map; left out");
        NOT_UNDERSTOOD.with(|count| count.set(count.get() + 1));
        return Ok(BTreeMap::new());
    };
    let total = entries.len();
    let readable: BTreeMap<String, T> = entries
        .into_iter()
        .filter_map(|(model, value)| Some((model, T::deserialize(value).ok()?)))
        .collect();
    let left_out = total - readable.len();
    if left_out > 0 {
        log::warn!("{left_out} {what} are not understood; left out");
        NOT_UNDERSTOOD.with(|count| count.set(count.get() + left_out));
    }
    Ok(readable)
}

/// What loading found: the app tells the user, and saves only what it may.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadOutcome {
    /// No file yet, or a file read in full.
    Read,
    /// Values not understood or of a newer version fell back to defaults; a copy of the file is
    /// kept next to it.
    PartlyUnderstood,
    /// Unreadable or malformed: moved aside, defaults in use.
    SetAside,
    /// Unreadable or not understood, and it could not be moved or copied aside: saving would
    /// overwrite it with what this run holds, so it must not be saved.
    Untouchable,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    pub settings: Settings,
    pub outcome: LoadOutcome,
}

/// The settings for a reader that does not save them.
pub fn load(path: &Path) -> Settings {
    load_checked(path).settings
}

/// Loads the settings; a missing file yields the defaults. An unreadable or malformed file is
/// moved aside (never overwriting an earlier one); one with values this version does not
/// understand is kept, with a copy next to it.
pub fn load_checked(path: &Path) -> Loaded {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return loaded(Settings::default(), LoadOutcome::Read);
        }
        Err(error) => {
            log::warn!(
                "settings file could not be read ({}); using defaults",
                error.kind()
            );
            return set_aside(path);
        }
    };
    NOT_UNDERSTOOD.with(|count| count.set(0));
    let settings: Settings = match serde_json::from_slice(&bytes) {
        Ok(settings) => settings,
        Err(error) => {
            // Only the error category is logged: the message can quote file contents,
            // which include speaker addresses.
            log::warn!(
                "settings file is malformed ({:?}); using defaults",
                error.classify()
            );
            return set_aside(path);
        }
    };
    let dropped = dropped_on_save(&bytes, &settings);
    if dropped > 0 {
        log::warn!("{dropped} settings values of a newer version would be dropped on saving");
    }
    if NOT_UNDERSTOOD.with(Cell::get) + dropped == 0 {
        return loaded(settings, LoadOutcome::Read);
    }
    let outcome = match keep_a_copy(path) {
        true => LoadOutcome::PartlyUnderstood,
        false => LoadOutcome::Untouchable,
    };
    loaded(settings, outcome)
}

fn loaded(settings: Settings, outcome: LoadOutcome) -> Loaded {
    Loaded { settings, outcome }
}

fn set_aside(path: &Path) -> Loaded {
    let outcome = match quarantine(path) {
        true => LoadOutcome::SetAside,
        false => LoadOutcome::Untouchable,
    };
    loaded(Settings::default(), outcome)
}

/// How many values of the file `bytes` saving `settings` would lose: keys this version does not
/// know, at any depth.
fn dropped_on_save(bytes: &[u8], settings: &Settings) -> usize {
    let Ok(original) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return 0;
    };
    let Ok(written) = serde_json::to_value(settings) else {
        return 0;
    };
    dropped_keys(&original, &written)
}

fn dropped_keys(original: &serde_json::Value, written: &serde_json::Value) -> usize {
    use serde_json::Value;
    match (original, written) {
        (Value::Object(original), Value::Object(written)) => original
            .iter()
            .map(|(key, value)| match written.get(key) {
                Some(kept) => dropped_keys(value, kept),
                None => usize::from(carries_something(value)),
            })
            .sum(),
        (Value::Array(original), Value::Array(written)) => original
            .iter()
            .zip(written)
            .map(|(value, kept)| dropped_keys(value, kept))
            .sum(),
        _ => 0,
    }
}

/// Empty lists and maps and `null` are what this version leaves out itself.
fn carries_something(value: &serde_json::Value) -> bool {
    use serde_json::Value;
    match value {
        Value::Null => false,
        Value::Array(items) => !items.is_empty(),
        Value::Object(entries) => !entries.is_empty(),
        _ => true,
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
/// Whether the copy was made.
fn keep_a_copy(path: &Path) -> bool {
    let target = free_quarantine_path(path);
    let copied = fs::copy(path, &target);
    if let Err(error) = &copied {
        log::warn!("settings file could not be copied aside ({})", error.kind());
    }
    copied.is_ok()
}

/// Whether the file was moved aside.
fn quarantine(path: &Path) -> bool {
    let target = free_quarantine_path(path);
    let moved = fs::rename(path, &target);
    if let Err(error) = &moved {
        log::warn!("settings file could not be moved aside ({})", error.kind());
    }
    moved.is_ok()
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
