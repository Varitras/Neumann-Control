//! UI-independent state of the speaker cards: what the worker events add up to, which targets
//! to talk to, and the time-based behaviour of meters and the fader wheel.
//! All times are seconds of the UI clock passed in by the caller.

use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddrV6;
use std::ops::RangeInclusive;

use neumann_control_core::address::parse_address;
use neumann_control_core::catalog::Catalog;
use neumann_control_core::discovery::Found;
use neumann_control_core::mask::{mask_host, mask_serial};
use neumann_control_core::settings::ManualSpeaker;
use neumann_control_core::speaker::{Event, Identity};
use neumann_control_core::ssc::wire::SscPath;
use serde_json::Value;

use crate::texts;

pub const PEAK_HOLD_SECONDS: f64 = 1.5;
pub const METER_FLOOR_DB: f64 = -60.0;
pub const METER_CEILING_DB: f64 = 6.0;
pub const WHEEL_STEP_DB: f64 = 0.5;
pub const WHEEL_IDLE_SECONDS: f64 = 0.3;

const INPUT_METER_PATH: &str = "m/in/level";
const OUTPUT_METER_PATH: &str = "m/out/level";
pub const OUTPUT_LEVEL_PATH: &str = "audio/out/level";

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SpeakerKey {
    Serial(String),
    Address(SocketAddrV6),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Connecting,
    Connected,
    Unreachable,
}

#[derive(Clone, Debug)]
pub struct SpeakerState {
    pub key: SpeakerKey,
    pub address: SocketAddrV6,
    pub identity: Option<Identity>,
    /// What the last scan reported for this speaker; names may be empty.
    pub discovered: Option<Found>,
    pub values: BTreeMap<SscPath, Value>,
    pub absent: BTreeSet<SscPath>,
    pub status: Status,
    pub message: Option<String>,
    /// The path a write message is about; a fresh value for it clears the message.
    message_path: Option<SscPath>,
    pub expanded: bool,
    pub input_meters: Vec<MeterState>,
    pub output_meters: Vec<MeterState>,
    pub fader: FaderState,
}

impl SpeakerState {
    pub fn new(key: SpeakerKey, address: SocketAddrV6) -> SpeakerState {
        SpeakerState {
            key,
            address,
            identity: None,
            discovered: None,
            values: BTreeMap::new(),
            absent: BTreeSet::new(),
            status: Status::Connecting,
            message: None,
            message_path: None,
            expanded: false,
            input_meters: Vec::new(),
            output_meters: Vec::new(),
            fader: FaderState::default(),
        }
    }

    /// The speaker's own name, else the announced one, else the model, else the masked
    /// address; mDNS names and device names can both be empty.
    pub fn display_name(&self) -> String {
        let own = self
            .identity
            .as_ref()
            .map(|identity| identity.name.as_str());
        let announced = self.discovered.as_ref().map(|found| found.name.as_str());
        [own, announced, self.model()]
            .into_iter()
            .flatten()
            .find(|name| !name.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| mask_host(&self.address))
    }

    /// The reported product, else the announced one.
    pub fn model(&self) -> Option<&str> {
        let reported = self
            .identity
            .as_ref()
            .map(|identity| identity.product.as_str());
        let announced = self
            .discovered
            .as_ref()
            .and_then(|found| found.product.as_deref());
        [reported, announced]
            .into_iter()
            .flatten()
            .find(|model| !model.is_empty())
    }

    pub fn apply(&mut self, event: Event, now: f64) {
        match event {
            Event::Connected(identity) => {
                if !identity.serial.is_empty() {
                    self.key = SpeakerKey::Serial(identity.serial.clone());
                }
                self.identity = Some(identity);
                self.status = Status::Connected;
                self.set_message(None, None);
            }
            // The masked reason is for the log; the card only shows the generic text.
            Event::Unreachable(_) => {
                self.status = Status::Unreachable;
                self.set_message(Some(texts::UNREACHABLE.to_owned()), None);
                self.reset_meters();
            }
            Event::Value { path, value } => {
                if self.message_path.as_ref() == Some(&path) {
                    self.set_message(None, None);
                }
                self.absent.remove(&path);
                self.feed_meters(&path, &value, now);
                self.values.insert(path, value);
            }
            Event::Absent(path) => {
                self.values.remove(&path);
                self.absent.insert(path);
            }
            Event::WriteRejected { path, error } => {
                let text = texts::describe_rejection(error.code);
                self.set_message(Some(text), Some(path));
            }
            Event::WriteFailed { path, .. } => {
                self.set_message(Some(texts::WRITE_FAILED.to_owned()), Some(path));
            }
            Event::SerialMismatch { found, .. } => {
                self.status = Status::Unreachable;
                let text = texts::serial_mismatch(&mask_serial(&found));
                self.set_message(Some(text), None);
            }
            // The full-value dump has no state to land in until the report view exists.
            Event::AllValues(_) | Event::AllValuesFailed(_) => {}
        }
    }

    fn set_message(&mut self, message: Option<String>, path: Option<SscPath>) {
        self.message = message;
        self.message_path = path;
    }

    fn reset_meters(&mut self) {
        let meters = self.input_meters.iter_mut().chain(&mut self.output_meters);
        for meter in meters {
            meter.reset();
        }
    }

    fn feed_meters(&mut self, path: &SscPath, value: &Value, now: f64) {
        let meters = match path.to_string().as_str() {
            INPUT_METER_PATH => &mut self.input_meters,
            OUTPUT_METER_PATH => &mut self.output_meters,
            _ => return,
        };
        let Some(levels) = value.as_array() else {
            return;
        };
        if meters.len() < levels.len() {
            meters.resize_with(levels.len(), MeterState::default);
        }
        for (meter, level) in meters.iter_mut().zip(levels) {
            meter.update(level.as_f64().unwrap_or(METER_FLOOR_DB), now);
        }
    }
}

/// Manual entries first (settings order), then discovered speakers not yet listed. A speaker
/// present on both sides (same serial, or same address) is one target at the discovered
/// address, keyed by the serial once either side knows it. Manual entries whose address does not parse are skipped here and stay
/// untouched in the settings.
pub fn merge_targets(manual: &[ManualSpeaker], found: &[Found]) -> Vec<(SpeakerKey, SocketAddrV6)> {
    let mut targets: Vec<(SpeakerKey, SocketAddrV6)> = Vec::new();
    for speaker in manual {
        let Ok(address) = parse_address(&speaker.address, speaker.port) else {
            continue;
        };
        let key = match &speaker.serial {
            Some(serial) => SpeakerKey::Serial(serial.clone()),
            None => SpeakerKey::Address(address),
        };
        push_or_replace(&mut targets, key, address, false);
    }
    for entry in found {
        let key = match &entry.serial {
            Some(serial) => SpeakerKey::Serial(serial.clone()),
            None => SpeakerKey::Address(entry.address),
        };
        push_or_replace(&mut targets, key, entry.address, true);
    }
    targets
}

fn push_or_replace(
    targets: &mut Vec<(SpeakerKey, SocketAddrV6)>,
    key: SpeakerKey,
    address: SocketAddrV6,
    from_found: bool,
) {
    let position = targets
        .iter()
        .position(|(existing, existing_address)| *existing == key || *existing_address == address);
    let Some(position) = position else {
        targets.push((key, address));
        return;
    };
    if !from_found {
        return;
    }
    if matches!(key, SpeakerKey::Serial(_)) {
        targets[position].0 = key;
    }
    targets[position].1 = address;
}

/// The catalog range of the output level, which the wheel fader drives.
pub fn output_level_range() -> RangeInclusive<f64> {
    let path = SscPath::parse(OUTPUT_LEVEL_PATH).expect("constant path");
    let entry = Catalog::embedded()
        .entries
        .iter()
        .find(|entry| entry.path == path)
        .expect("catalog has the output level");
    let min = entry.min.expect("output level has a minimum");
    let max = entry.max.expect("output level has a maximum");
    min..=max
}

/// Maps a dB value linearly from the floor..ceiling range to 0..1; the device reports silence
/// as -160, which lands below the floor and gives 0.
pub fn fraction(db: f64) -> f32 {
    let position = (db - METER_FLOOR_DB) / (METER_CEILING_DB - METER_FLOOR_DB);
    position.clamp(0.0, 1.0) as f32
}

#[derive(Clone, Copy, Debug)]
pub struct MeterState {
    level: f64,
    peak: f64,
    peak_since: f64,
}

impl Default for MeterState {
    fn default() -> MeterState {
        MeterState {
            level: METER_FLOOR_DB,
            peak: METER_FLOOR_DB,
            peak_since: 0.0,
        }
    }
}

impl MeterState {
    pub fn update(&mut self, level: f64, now: f64) {
        self.level = level;
        let hold_expired = now - self.peak_since >= PEAK_HOLD_SECONDS;
        if level >= self.peak || hold_expired {
            self.peak = level;
            self.peak_since = now;
        }
    }

    pub fn level(&self) -> f64 {
        self.level
    }

    /// The held peak, or the current level once the hold ran out.
    pub fn peak(&self, now: f64) -> f64 {
        if now - self.peak_since >= PEAK_HOLD_SECONDS {
            return self.level;
        }
        self.peak
    }

    pub fn reset(&mut self) {
        *self = MeterState::default();
    }
}

/// Collects mouse-wheel steps so one write goes out after the wheel went quiet.
#[derive(Clone, Copy, Debug, Default)]
pub struct FaderState {
    pending: Option<f64>,
    last_step: f64,
}

impl FaderState {
    /// `range` is the allowed value range of the faded entry.
    pub fn on_wheel(&mut self, steps: i32, current: f64, range: RangeInclusive<f64>, now: f64) {
        let base = self.pending.unwrap_or(current);
        let target = base + f64::from(steps) * WHEEL_STEP_DB;
        self.pending = Some(target.clamp(*range.start(), *range.end()));
        self.last_step = now;
    }

    pub fn due(&mut self, now: f64) -> Option<f64> {
        let idle = now - self.last_step >= WHEEL_IDLE_SECONDS;
        if self.pending.is_some() && idle {
            return self.pending.take();
        }
        None
    }

    /// When the pending wheel value becomes due, so the UI can wake up for it.
    pub fn deadline(&self) -> Option<f64> {
        self.pending.map(|_| self.last_step + WHEEL_IDLE_SECONDS)
    }

    pub fn shown(&self, device: f64) -> f64 {
        self.pending.unwrap_or(device)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neumann_control_core::ssc::wire::DeviceError;
    use serde_json::json;

    const SERIAL: &str = "SIM0001234";
    const OTHER_SERIAL: &str = "SIM0007500";
    const ADDRESS_TEXT: &str = "fe80::2a36:38ff:fe12:3456%7";
    const OTHER_ADDRESS_TEXT: &str = "fe80::2a36:38ff:fe12:3457%7";
    const PORT: u16 = 45;

    fn address(text: &str) -> SocketAddrV6 {
        parse_address(text, PORT).unwrap()
    }

    fn manual(text: &str, serial: Option<&str>) -> ManualSpeaker {
        ManualSpeaker {
            address: text.to_owned(),
            port: PORT,
            serial: serial.map(str::to_owned),
        }
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
    fn same_serial_merges_into_one_target() {
        let targets = merge_targets(
            &[manual(ADDRESS_TEXT, Some(SERIAL))],
            &[found(OTHER_ADDRESS_TEXT, Some(SERIAL))],
        );
        assert_eq!(
            targets,
            vec![(
                SpeakerKey::Serial(SERIAL.to_owned()),
                address(OTHER_ADDRESS_TEXT)
            )]
        );
    }

    #[test]
    fn manual_without_serial_stays_by_address() {
        let targets = merge_targets(
            &[manual(ADDRESS_TEXT, None), manual("not an address", None)],
            &[found(OTHER_ADDRESS_TEXT, Some(OTHER_SERIAL))],
        );
        assert_eq!(
            targets,
            vec![
                (
                    SpeakerKey::Address(address(ADDRESS_TEXT)),
                    address(ADDRESS_TEXT)
                ),
                (
                    SpeakerKey::Serial(OTHER_SERIAL.to_owned()),
                    address(OTHER_ADDRESS_TEXT)
                ),
            ]
        );
    }

    #[test]
    fn found_without_serial_dedupes_by_address() {
        let targets = merge_targets(&[manual(ADDRESS_TEXT, None)], &[found(ADDRESS_TEXT, None)]);
        assert_eq!(targets.len(), 1);
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
    fn unreachable_event_greys_card_and_keeps_values() {
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
        assert_eq!(speaker.message.as_deref(), Some(texts::UNREACHABLE));
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
        assert_eq!(
            speaker.message.as_deref(),
            Some("Gerät lehnt ab: nur lesbar")
        );
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
        let message = speaker.message.unwrap();
        assert!(message.contains(&mask_serial(OTHER_SERIAL)));
        assert!(!message.contains(OTHER_SERIAL));
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
    fn peak_holds_then_falls_after_1_5_s() {
        let mut meter = MeterState::default();
        meter.update(-6.0, 0.0);
        meter.update(-30.0, 1.0);
        assert_eq!(meter.peak(1.0), -6.0);
        assert_eq!(meter.level(), -30.0);
        assert_eq!(meter.peak(1.25), -6.0);
        assert_eq!(meter.peak(1.5), -30.0);
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
    fn unreachable_resets_meters() {
        let mut speaker = state();
        speaker.apply(value_event("m/out/level", json!([-3.0])), 0.0);
        speaker.apply(Event::Unreachable("masked".to_owned()), 0.5);
        assert_eq!(speaker.output_meters[0].level(), METER_FLOOR_DB);
        assert_eq!(speaker.output_meters[0].peak(0.5), METER_FLOOR_DB);
    }

    #[test]
    fn write_message_clears_when_the_path_reports_a_value() {
        let mut speaker = state();
        speaker.apply(
            Event::WriteFailed {
                path: path("audio/out/level"),
                message: "lost".to_owned(),
            },
            0.0,
        );
        assert_eq!(speaker.message.as_deref(), Some(texts::WRITE_FAILED));
        speaker.apply(value_event("device/name", json!("left")), 1.0);
        assert!(speaker.message.is_some());
        speaker.apply(value_event("audio/out/level", json!(80.0)), 2.0);
        assert_eq!(speaker.message, None);
    }

    #[test]
    fn manual_address_and_found_serial_merge_into_one_target() {
        let targets = merge_targets(
            &[manual(ADDRESS_TEXT, None)],
            &[found(ADDRESS_TEXT, Some(SERIAL))],
        );
        assert_eq!(
            targets,
            vec![(SpeakerKey::Serial(SERIAL.to_owned()), address(ADDRESS_TEXT))]
        );
    }

    #[test]
    fn meter_fraction_clamps_below_floor_and_above_ceiling() {
        assert_eq!(fraction(-160.0), 0.0);
        assert_eq!(fraction(10.0), 1.0);
        assert_eq!(fraction(-27.0), 0.5);
    }

    #[test]
    fn wheel_steps_debounce_to_one_value() {
        let range = output_level_range();
        let mut fader = FaderState::default();
        fader.on_wheel(1, 80.0, range.clone(), 1.0);
        fader.on_wheel(1, 80.0, range.clone(), 1.125);
        fader.on_wheel(1, 80.0, range, 1.25);
        assert_eq!(fader.shown(80.0), 81.5);
        assert_eq!(fader.due(1.5), None);
        assert_eq!(fader.due(1.625), Some(81.5));
        assert_eq!(fader.due(2.0), None);
        assert_eq!(fader.shown(80.0), 80.0);
    }

    #[test]
    fn wheel_deadline_follows_the_last_step_and_ends_when_due() {
        let range = output_level_range();
        let mut fader = FaderState::default();
        assert_eq!(fader.deadline(), None);
        fader.on_wheel(1, 80.0, range.clone(), 1.0);
        fader.on_wheel(1, 80.0, range, 1.5);
        assert_eq!(fader.deadline(), Some(1.5 + WHEEL_IDLE_SECONDS));
        fader.due(2.0);
        assert_eq!(fader.deadline(), None);
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

    #[test]
    fn wheel_clamps_at_120() {
        let range = output_level_range();
        assert_eq!(range, 0.0..=120.0);
        let mut fader = FaderState::default();
        fader.on_wheel(10, 119.0, range.clone(), 0.0);
        assert_eq!(fader.due(1.0), Some(120.0));
        fader.on_wheel(-1000, 1.0, range, 2.0);
        assert_eq!(fader.due(3.0), Some(0.0));
    }
}
