//! UI-independent state of one speaker: what the worker events add up to.
//! All times are seconds of the UI clock passed in by the caller.

use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddrV6;
use std::time::Duration;

use neumann_control_core::discovery::Found;
use neumann_control_core::mask::{mask_host, mask_serial, mask_serial_in, mask_serial_like};
use neumann_control_core::schema_walk::LeafReport;
use neumann_control_core::settings::KnownSpeaker;
use neumann_control_core::speaker::{Command, Event, Identity};
use neumann_control_core::ssc::wire::SscPath;
use serde_json::Value;

use crate::display::capped;
use crate::gesture::GestureState;
use crate::history::{is_given, matching, merged};
use crate::meter::{MeterState, METER_FLOOR_DB};
use crate::paths;
use crate::status_leds::active_warnings;
use crate::trend::Trend;
use crate::writes::Refusal;

/// The KH 750 reports three output channels; a longer list from a faulty device would draw a
/// meter per element.
pub const MAX_METER_CHANNELS: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SpeakerKey {
    Serial(String),
    Address(SocketAddrV6),
}

/// "Komplett scannen": the walk, then its report file.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum FullScanState {
    #[default]
    Idle,
    Running,
    /// Read; the app writes the report and replaces this with the outcome.
    Read(Vec<LeafReport>),
    /// The JSON file's path.
    Written(String),
    Failed(String),
}

/// What the speaker's message is about, which decides when it ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MessageSource {
    /// Connecting, unreachable, another device: replaced by the next connection event.
    Connection,
    /// A refused, rejected or failed write. It stays until the user's next write request on
    /// this speaker (or a connection event): the worker re-reads a path right after a rejection
    /// and polls it every few seconds, so ending it on a fresh value hid it at once; and the
    /// texts do not name the path, so they are stale after any later request.
    Write,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Connecting,
    Connected,
    Unreachable,
}

/// The answer to "Alle Werte…": every value the speaker has, read on request.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum AllValuesState {
    #[default]
    Idle,
    Loading,
    Loaded(Vec<LeafReport>),
    Failed(String),
}

/// What the strip says about the speaker; worded only when drawn, so it follows the language.
#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    Unreachable,
    WriteFailed,
    /// The device refused a write with this error code.
    Rejected(i64),
    /// A different device answered at the address; its serial, masked.
    SerialMismatch(String),
    Refused(Refusal),
}

#[derive(Clone, Debug)]
pub struct SpeakerState {
    pub key: SpeakerKey,
    pub address: SocketAddrV6,
    pub identity: Option<Identity>,
    /// What the last scan reported for this speaker; names may be empty.
    pub discovered: Option<Found>,
    /// What it reported when it last connected, from the settings; names it while it is off.
    pub remembered: Option<KnownSpeaker>,
    pub values: BTreeMap<SscPath, Value>,
    pub absent: BTreeSet<SscPath>,
    pub status: Status,
    /// When the current outage began (the UI clock); `None` while connected or connecting.
    pub unreachable_since: Option<f64>,
    pub message: Option<Message>,
    message_source: MessageSource,
    pub input_meters: Vec<MeterState>,
    pub output_meters: Vec<MeterState>,
    pub fader: GestureState,
    /// One per knob the user turned, so a sent value stays shown until the speaker reports it.
    pub knobs: BTreeMap<SscPath, GestureState>,
    pub all_values: AllValuesState,
    /// Case-insensitive part of a path the "Alle Werte" table is narrowed to.
    pub all_values_filter: String,
    /// The worker's running mean; none until the current connection measured one.
    pub response_time: Option<Duration>,
    /// What this frame sent: path, value before, value written. The app takes it into the
    /// history at the end of the frame.
    pub written: Vec<(SscPath, Option<Value>, Value)>,
    /// Sent values the speaker has not answered yet; the value before of the next write.
    pending: BTreeMap<SscPath, Pending>,
    /// `device/temperature` as reported (kelvin), for the device panel's trend.
    pub temperature: Trend<f64>,
    /// The active warnings, empty while there are none.
    pub warnings: Trend<Vec<String>>,
    pub full_scan: FullScanState,
}

/// The newest value sent to one path and how many of its `Set`s are still unanswered.
#[derive(Clone, Debug, PartialEq)]
struct Pending {
    value: Value,
    unanswered: usize,
}

impl SpeakerState {
    pub fn new(key: SpeakerKey, address: SocketAddrV6) -> SpeakerState {
        SpeakerState {
            key,
            address,
            identity: None,
            discovered: None,
            remembered: None,
            values: BTreeMap::new(),
            absent: BTreeSet::new(),
            status: Status::Connecting,
            unreachable_since: None,
            message: None,
            message_source: MessageSource::Connection,
            input_meters: Vec::new(),
            output_meters: Vec::new(),
            fader: GestureState::default(),
            knobs: BTreeMap::new(),
            all_values: AllValuesState::Idle,
            all_values_filter: String::new(),
            response_time: None,
            written: Vec::new(),
            pending: BTreeMap::new(),
            temperature: Trend::default(),
            warnings: Trend::default(),
            full_scan: FullScanState::Idle,
        }
    }

    /// The speaker's own name, else the announced one, else the one it had when it last
    /// connected, else the model, else the masked address; any of these names can be empty.
    pub fn display_name(&self) -> String {
        let own = self
            .identity
            .as_ref()
            .map(|identity| identity.name.as_str());
        let announced = self.discovered.as_ref().map(|found| found.name.as_str());
        let remembered = self.remembered.as_ref().map(|known| known.name.as_str());
        [own, announced, remembered, self.model()]
            .into_iter()
            .flatten()
            .find(|name| !name.is_empty())
            .map(|name| capped(&self.with_serial_masked(name)))
            .unwrap_or_else(|| mask_host(&self.address))
    }

    /// A name may hold the serial (factory names do); it is shown masked like the announced
    /// name. Without a known serial, digits as long as a serial are masked.
    fn with_serial_masked(&self, name: &str) -> String {
        let serial = self
            .identity
            .as_ref()
            .map(|identity| identity.serial.as_str())
            .or(match &self.key {
                SpeakerKey::Serial(serial) => Some(serial.as_str()),
                SpeakerKey::Address(_) => None,
            })
            .filter(|serial| !serial.is_empty());
        match serial {
            Some(serial) => mask_serial_in(name, serial),
            None => mask_serial_like(name),
        }
    }

    /// The product the speaker itself reported. Writes and the catalog rows use only this,
    /// like the worker's polling: the announced model is unchecked mDNS text.
    pub fn product(&self) -> &str {
        self.identity
            .as_ref()
            .map_or("", |identity| identity.product.as_str())
    }

    /// The reported product, else the announced one; for showing only.
    pub fn model(&self) -> Option<&str> {
        let reported = self
            .identity
            .as_ref()
            .map(|identity| identity.product.as_str());
        let announced = self
            .discovered
            .as_ref()
            .and_then(|found| found.product.as_deref());
        let remembered = self.remembered.as_ref().map(|known| known.product.as_str());
        [reported, announced, remembered]
            .into_iter()
            .flatten()
            .find(|model| !model.is_empty())
    }

    /// Notes a sent write for the history; Identify is a momentary signal, not a setting.
    pub(crate) fn record_written(&mut self, path: &SscPath, value: &Value) {
        if *path == paths::path(paths::IDENTIFY) {
            return;
        }
        let base = self.latest(path);
        let before = base.map(|base| matching(base, value)).filter(is_given);
        let merged = merged(base, value);
        let unanswered = self
            .pending
            .get(path)
            .map_or(0, |pending| pending.unanswered);
        self.written.push((path.clone(), before, value.clone()));
        let pending = Pending {
            value: merged,
            unanswered: unanswered + 1,
        };
        self.pending.insert(path.clone(), pending);
    }

    /// The newest sent value not yet answered, else the reported one: what the speaker holds
    /// once its writes are through. The next toggle and a take-back start from this, so a write
    /// still on the way does not count as someone else's change.
    pub fn latest(&self, path: &SscPath) -> Option<&Value> {
        let sent = self.pending.get(path).map(|pending| &pending.value);
        sent.or_else(|| self.values.get(path))
    }

    /// Counts one `Set` to `path` as answered; when none is left, the sent value is no longer
    /// held. Whether the speaker took it, its next report says.
    fn settle(&mut self, path: &SscPath) {
        if let Some(pending) = self.pending.get_mut(path) {
            pending.unanswered = pending.unanswered.saturating_sub(1);
            if pending.unanswered > 0 {
                return;
            }
            self.pending.remove(path);
        }
        if *path == paths::path(paths::OUTPUT_LEVEL) {
            self.fader.confirmed();
        }
        if let Some(knob) = self.knobs.get_mut(path) {
            knob.confirmed();
        }
    }

    fn take_reported(&mut self, path: SscPath, value: Value, now: f64) {
        self.absent.remove(&path);
        self.feed_meters(&path, &value, now);
        self.record_trends(&path, &value, now);
        self.values.insert(path, value);
    }

    pub fn apply(&mut self, event: Event, now: f64) {
        match event {
            Event::Connected(identity) => {
                if !identity.serial.is_empty() {
                    self.key = SpeakerKey::Serial(identity.serial.clone());
                }
                self.identity = Some(identity);
                self.status = Status::Connected;
                self.unreachable_since = None;
                self.set_message(None, MessageSource::Connection);
            }
            // The masked reason is for the log; the strip only shows the generic text.
            Event::Unreachable(_) => {
                self.status = Status::Unreachable;
                self.unreachable_since.get_or_insert(now);
                self.set_message(Some(Message::Unreachable), MessageSource::Connection);
                self.reset_meters();
                self.response_time = None;
                self.pending.clear();
                self.temperature.record(now, None);
                self.warnings.record(now, None);
            }
            // The worker answers one request at a time, so a read answered before a write was
            // sent can arrive after it; the write's own answer follows and wins.
            Event::Value { path, value } => {
                if !self.pending.contains_key(&path) {
                    self.take_reported(path, value, now);
                }
            }
            Event::Written { path, value } => {
                self.settle(&path);
                if !self.pending.contains_key(&path) {
                    self.take_reported(path, value, now);
                }
            }
            Event::Absent(path) => {
                // Every later write to the path is answered the same way, so none is awaited.
                self.pending.remove(&path);
                self.settle(&path);
                self.values.remove(&path);
                self.absent.insert(path);
            }
            // A re-read or, for a lost connection, the reconnect follows; it shows the real value.
            Event::WriteRejected { path, error } => {
                self.settle(&path);
                let message = Message::Rejected(error.code);
                self.set_message(Some(message), MessageSource::Write);
            }
            Event::WriteFailed { path, .. } => {
                self.settle(&path);
                self.set_message(Some(Message::WriteFailed), MessageSource::Write);
            }
            Event::SerialMismatch { found, .. } => {
                self.status = Status::Unreachable;
                self.unreachable_since.get_or_insert(now);
                self.response_time = None;
                self.pending.clear();
                let message = Message::SerialMismatch(mask_serial(&found));
                self.set_message(Some(message), MessageSource::Connection);
                self.temperature.record(now, None);
                self.warnings.record(now, None);
            }
            Event::AllValues(reports) => {
                if self.full_scan == FullScanState::Running {
                    self.full_scan = FullScanState::Read(reports.clone());
                }
                self.finish_all_values(AllValuesState::Loaded(reports));
            }
            Event::AllValuesFailed(reason) => {
                if self.full_scan == FullScanState::Running {
                    self.full_scan = FullScanState::Failed(reason.clone());
                }
                self.finish_all_values(AllValuesState::Failed(reason));
            }
            Event::ResponseTime(time) => self.response_time = Some(time),
        }
    }

    fn record_trends(&mut self, path: &SscPath, value: &Value, now: f64) {
        if *path == paths::path(paths::TEMPERATURE) {
            self.temperature.record(now, value.as_f64());
        } else if *path == paths::path(paths::WARNINGS) {
            self.warnings.record(now, active_warnings(value));
        }
    }

    /// Starts reading every value; the worker answers with `AllValues` or `AllValuesFailed`.
    pub fn read_all_values(&mut self) -> Command {
        self.all_values = AllValuesState::Loading;
        Command::ReadAll
    }

    /// Starts "Komplett scannen"; the walk is the same as for "Alle Werte".
    pub fn start_full_scan(&mut self) -> Command {
        self.full_scan = FullScanState::Running;
        Command::ReadAll
    }

    /// Closes the "Alle Werte" window and forgets its filter.
    pub fn close_all_values(&mut self) {
        self.all_values = AllValuesState::Idle;
        self.all_values_filter.clear();
    }

    /// An answer that comes after the window was closed would reopen it.
    fn finish_all_values(&mut self, outcome: AllValuesState) {
        if self.all_values == AllValuesState::Loading {
            self.all_values = outcome;
        }
    }

    /// The loudest input channel as its meter shows it now; none before any meter reading.
    pub fn loudest_input(&self, now: f64) -> Option<f64> {
        self.input_meters
            .iter()
            .map(|meter| meter.shown(now))
            .reduce(f64::max)
    }

    pub(crate) fn set_message(&mut self, message: Option<Message>, source: MessageSource) {
        self.message = message;
        self.message_source = source;
    }

    /// A new write request makes the message about the previous one stale.
    pub(crate) fn clear_write_message(&mut self) {
        if self.message_source == MessageSource::Write {
            self.set_message(None, MessageSource::Write);
        }
    }

    fn reset_meters(&mut self) {
        let meters = self.input_meters.iter_mut().chain(&mut self.output_meters);
        for meter in meters {
            meter.reset();
        }
    }

    fn feed_meters(&mut self, path: &SscPath, value: &Value, now: f64) {
        let meters = match path.to_string().as_str() {
            paths::INPUT_METER => &mut self.input_meters,
            paths::OUTPUT_METER => &mut self.output_meters,
            _ => return,
        };
        let Some(levels) = value.as_array() else {
            return;
        };
        let levels = &levels[..levels.len().min(MAX_METER_CHANNELS)];
        if meters.len() < levels.len() {
            meters.resize_with(levels.len(), MeterState::default);
        }
        for (meter, level) in meters.iter_mut().zip(levels) {
            meter.update(level.as_f64().unwrap_or(METER_FLOOR_DB), now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::VALUE_CHARACTER_LIMIT;
    use neumann_control_core::address::parse_address;
    use neumann_control_core::ssc::wire::DeviceError;
    use serde_json::json;

    const SERIAL: &str = "SIM0001234";
    const OTHER_SERIAL: &str = "SIM0007500";
    const ADDRESS_TEXT: &str = "fe80::2a36:38ff:fe12:3456%7";
    const PORT: u16 = 45;

    fn address(text: &str) -> SocketAddrV6 {
        parse_address(text, PORT).unwrap()
    }

    fn found(text: &str, serial: Option<&str>) -> Found {
        Found {
            name: "KH 120".to_owned(),
            product: None,
            serial: serial.map(str::to_owned),
            address: address(text),
        }
    }

    fn state() -> SpeakerState {
        SpeakerState::new(
            SpeakerKey::Address(address(ADDRESS_TEXT)),
            address(ADDRESS_TEXT),
        )
    }

    fn value_event(text: &str, value: Value) -> Event {
        Event::Value {
            path: path(text),
            value,
        }
    }

    fn path(text: &str) -> SscPath {
        SscPath::parse(text).unwrap()
    }

    #[test]
    fn temperature_and_warnings_are_recorded_with_gaps() {
        let mut speaker = state();
        speaker.apply(value_event("device/temperature", json!(310)), 1.0);
        speaker.apply(value_event("warnings", json!(["NO_WARNING"])), 1.0);
        speaker.apply(value_event("warnings", json!(["OVERTEMPERATURE"])), 2.0);
        speaker.apply(Event::Unreachable("gone".to_owned()), 3.0);
        let temperature: Vec<_> = speaker
            .temperature
            .points()
            .map(|point| (point.time, point.value))
            .collect();
        assert_eq!(temperature, vec![(1.0, Some(310.0)), (3.0, None)]);
        let warnings: Vec<_> = speaker
            .warnings
            .points()
            .map(|point| (point.time, point.value.clone()))
            .collect();
        let over = vec!["OVERTEMPERATURE".to_owned()];
        assert_eq!(
            warnings,
            vec![(1.0, Some(Vec::new())), (2.0, Some(over)), (3.0, None)]
        );
    }

    #[test]
    fn connected_upgrades_key_to_serial_and_keeps_address() {
        let mut speaker = state();
        speaker.apply(
            Event::Connected(Identity {
                product: "KH 120 II".to_owned(),
                version: "1".to_owned(),
                serial: SERIAL.to_owned(),
                name: "left".to_owned(),
            }),
            0.0,
        );
        assert_eq!(speaker.key, SpeakerKey::Serial(SERIAL.to_owned()));
        assert_eq!(speaker.address, address(ADDRESS_TEXT));
        assert_eq!(speaker.status, Status::Connected);
    }

    #[test]
    fn unreachable_event_greys_the_speaker_and_keeps_values() {
        let mut speaker = state();
        speaker.apply(
            Event::Value {
                path: path("device/name"),
                value: json!("left"),
            },
            0.0,
        );
        speaker.apply(Event::Unreachable("masked".to_owned()), 1.0);
        assert_eq!(speaker.status, Status::Unreachable);
        assert_eq!(speaker.message, Some(Message::Unreachable));
        assert_eq!(speaker.values.len(), 1);
    }

    #[test]
    fn absent_event_removes_value() {
        let mut speaker = state();
        speaker.apply(
            Event::Value {
                path: path("device/name"),
                value: json!("left"),
            },
            0.0,
        );
        speaker.apply(Event::Absent(path("device/name")), 1.0);
        assert!(speaker.values.is_empty());
        assert!(speaker.absent.contains(&path("device/name")));
    }

    #[test]
    fn write_rejected_sets_german_message() {
        let mut speaker = state();
        speaker.apply(
            Event::WriteRejected {
                path: path("device/name"),
                error: DeviceError {
                    code: 405,
                    description: String::new(),
                },
            },
            0.0,
        );
        assert_eq!(speaker.message, Some(Message::Rejected(405)));
    }

    #[test]
    fn serial_mismatch_masks_found_serial() {
        let mut speaker = state();
        speaker.apply(
            Event::SerialMismatch {
                expected: SERIAL.to_owned(),
                found: OTHER_SERIAL.to_owned(),
            },
            0.0,
        );
        assert_eq!(speaker.status, Status::Unreachable);
        let Some(Message::SerialMismatch(masked)) = speaker.message else {
            panic!("{:?}", speaker.message);
        };
        assert_eq!(masked, mask_serial(OTHER_SERIAL));
        assert_ne!(masked, OTHER_SERIAL);
    }

    #[test]
    fn response_time_is_cleared_when_unreachable() {
        let mut speaker = state();
        speaker.apply(Event::ResponseTime(Duration::from_millis(14)), 0.0);
        assert_eq!(speaker.response_time, Some(Duration::from_millis(14)));
        speaker.apply(Event::Unreachable("gone".to_owned()), 1.0);
        assert_eq!(speaker.response_time, None);
    }

    #[test]
    fn a_long_meter_list_draws_a_bounded_number_of_meters() {
        let mut speaker = state();
        let levels: Vec<f64> = vec![-20.0; 500];
        speaker.apply(
            Event::Value {
                path: path("m/in/level"),
                value: json!(levels),
            },
            0.0,
        );
        assert_eq!(speaker.input_meters.len(), MAX_METER_CHANNELS);
    }

    #[test]
    fn a_huge_device_name_is_shortened_for_the_strip() {
        let mut speaker = state();
        let huge = "n".repeat(1_000_000);
        speaker.apply(Event::Connected(identity(&huge, "KH 120 II")), 0.0);
        assert!(speaker.display_name().chars().count() <= VALUE_CHARACTER_LIMIT + 1);
    }

    #[test]
    fn meter_value_feeds_one_meter_per_channel() {
        let mut speaker = state();
        speaker.apply(
            Event::Value {
                path: path("m/out/level"),
                value: json!([-12.0, -20.5]),
            },
            0.0,
        );
        assert_eq!(speaker.output_meters.len(), 2);
        assert_eq!(speaker.output_meters[1].level(), -20.5);
    }

    #[test]
    fn input_and_output_meters_are_kept_apart() {
        let mut speaker = state();
        speaker.apply(value_event("m/in/level", json!([-10.0])), 0.0);
        speaker.apply(value_event("m/out/level", json!([-20.0, -30.0])), 0.0);
        assert_eq!(speaker.input_meters.len(), 1);
        assert_eq!(speaker.input_meters[0].level(), -10.0);
        assert_eq!(speaker.output_meters.len(), 2);
    }

    #[test]
    fn the_loudest_input_is_the_highest_channel_as_shown() {
        let mut speaker = state();
        assert_eq!(speaker.loudest_input(0.0), None);
        speaker.apply(value_event("m/in/level", json!([-30.0, -12.0])), 0.0);
        speaker.apply(value_event("m/out/level", json!([-3.0])), 0.0);
        // Long after the rise, the meters show the levels themselves.
        assert_eq!(speaker.loudest_input(10.0), Some(-12.0));
    }

    #[test]
    fn unreachable_resets_meters() {
        let mut speaker = state();
        speaker.apply(value_event("m/out/level", json!([-3.0])), 0.0);
        speaker.apply(Event::Unreachable("masked".to_owned()), 0.5);
        assert_eq!(speaker.output_meters[0].level(), METER_FLOOR_DB);
        assert_eq!(speaker.output_meters[0].peak(0.5), METER_FLOOR_DB);
    }

    #[test]
    fn a_rejection_message_survives_the_reread_of_its_path() {
        let mut speaker = state();
        speaker.apply(
            Event::WriteRejected {
                path: path("ui/bass_gain"),
                error: DeviceError {
                    code: 405,
                    description: String::new(),
                },
            },
            0.0,
        );
        speaker.apply(value_event("ui/bass_gain", json!(0.0)), 0.1);
        speaker.apply(value_event("ui/bass_gain", json!(0.0)), 30.0);
        assert_eq!(speaker.message, Some(Message::Rejected(405)));
    }

    #[test]
    fn a_failed_write_message_survives_values_of_its_path() {
        let mut speaker = state();
        speaker.apply(
            Event::WriteFailed {
                path: path("audio/out/level"),
                message: "lost".to_owned(),
            },
            0.0,
        );
        speaker.apply(value_event("audio/out/level", json!(80.0)), 2.0);
        assert_eq!(speaker.message, Some(Message::WriteFailed));
    }

    #[test]
    fn a_local_mode_rejection_names_the_rear_switch() {
        let mut speaker = state();
        speaker.apply(
            Event::WriteRejected {
                path: path("audio/out/level"),
                error: DeviceError {
                    code: 401,
                    description: "authorisation needed".to_owned(),
                },
            },
            0.0,
        );
        assert_eq!(speaker.message, Some(Message::Rejected(401)));
    }

    #[test]
    fn the_answer_to_the_level_write_ends_the_held_fader_value() {
        let mut speaker = state();
        let level = path(paths::OUTPUT_LEVEL);
        speaker.fader.sent(90.0, 0.0);
        speaker.record_written(&level, &json!(90.0));
        speaker.apply(value_event("device/name", json!("left")), 0.5);
        assert_eq!(speaker.fader.shown(80.0, 0.5), 90.0);
        let answer = Event::Written {
            path: level,
            value: json!(80.0),
        };
        speaker.apply(answer, 0.5);
        assert_eq!(speaker.fader.shown(80.0, 0.5), 80.0);
    }

    #[test]
    fn all_values_go_from_loading_to_loaded_or_failed() {
        let mut speaker = state();
        assert_eq!(speaker.read_all_values(), Command::ReadAll);
        assert_eq!(speaker.all_values, AllValuesState::Loading);
        let reports = vec![LeafReport {
            path: path("device/name"),
            value: Ok(json!("left")),
            limits: None,
        }];
        speaker.apply(Event::AllValues(reports.clone()), 0.0);
        assert_eq!(speaker.all_values, AllValuesState::Loaded(reports));
        speaker.read_all_values();
        speaker.apply(Event::AllValuesFailed("refused".to_owned()), 1.0);
        assert_eq!(
            speaker.all_values,
            AllValuesState::Failed("refused".to_owned())
        );
    }

    #[test]
    fn an_answer_after_closing_does_not_reopen_the_window() {
        let mut speaker = state();
        speaker.read_all_values();
        speaker.all_values_filter = "standby".to_owned();
        speaker.close_all_values();
        speaker.apply(Event::AllValues(Vec::new()), 0.0);
        assert_eq!(speaker.all_values, AllValuesState::Idle);
        assert!(speaker.all_values_filter.is_empty());
    }

    fn identity(name: &str, product: &str) -> Identity {
        Identity {
            product: product.to_owned(),
            version: "1".to_owned(),
            serial: SERIAL.to_owned(),
            name: name.to_owned(),
        }
    }

    #[test]
    fn display_name_falls_back_from_own_to_announced_to_model_to_masked_address() {
        let mut speaker = state();
        assert_eq!(speaker.display_name(), mask_host(&address(ADDRESS_TEXT)));
        speaker.discovered = Some(Found {
            name: String::new(),
            product: Some("KH 750".to_owned()),
            ..found(ADDRESS_TEXT, None)
        });
        assert_eq!(speaker.display_name(), "KH 750");
        speaker.discovered.as_mut().unwrap().name = "announced".to_owned();
        assert_eq!(speaker.display_name(), "announced");
        speaker.identity = Some(identity("", "KH 120 II"));
        assert_eq!(speaker.display_name(), "announced");
        speaker.identity = Some(identity("own", "KH 120 II"));
        assert_eq!(speaker.display_name(), "own");
    }

    #[test]
    fn model_prefers_the_reported_product() {
        let mut speaker = state();
        assert_eq!(speaker.model(), None);
        speaker.discovered = Some(Found {
            product: Some("KH 750".to_owned()),
            ..found(ADDRESS_TEXT, None)
        });
        assert_eq!(speaker.model(), Some("KH 750"));
        speaker.identity = Some(identity("", ""));
        assert_eq!(speaker.model(), Some("KH 750"));
        speaker.identity = Some(identity("", "KH 120 II"));
        assert_eq!(speaker.model(), Some("KH 120 II"));
    }

    /// A name that holds the serial is shown masked, as the announced one is.
    #[test]
    fn the_own_name_shows_its_serial_masked() {
        let address: SocketAddrV6 = "[fe80::1]:45".parse().unwrap();
        let mut speaker = SpeakerState::new(SpeakerKey::Address(address), address);
        let identity = Identity {
            product: "KH 120 II".to_owned(),
            version: "1".to_owned(),
            serial: "3712345678".to_owned(),
            name: "KH 120 II 3712345678".to_owned(),
        };
        speaker.apply(Event::Connected(identity), 0.0);
        assert!(!speaker.display_name().contains("3712345678"));
    }
}
