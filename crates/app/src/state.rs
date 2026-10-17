//! UI-independent state of the speaker cards: what the worker events add up to, which targets
//! to talk to, and the time-based behaviour of meters and the fader wheel.
//! All times are seconds of the UI clock passed in by the caller.

use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddrV6;

use neumann_control_core::address::parse_address;
use neumann_control_core::discovery::Found;
use neumann_control_core::mask::mask_serial;
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
pub const FADER_MIN_DB: f64 = 0.0;
pub const FADER_MAX_DB: f64 = 120.0;

const METER_PATHS: [&str; 2] = ["m/in/level", "m/out/level"];

#[derive(Clone, Debug, PartialEq, Eq)]
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
    pub values: BTreeMap<SscPath, Value>,
    pub absent: BTreeSet<SscPath>,
    pub status: Status,
    pub message: Option<String>,
    pub expanded: bool,
    pub meters: Vec<MeterState>,
    pub fader: FaderState,
}

impl SpeakerState {
    pub fn new(key: SpeakerKey, address: SocketAddrV6) -> SpeakerState {
        SpeakerState {
            key,
            address,
            identity: None,
            values: BTreeMap::new(),
            absent: BTreeSet::new(),
            status: Status::Connecting,
            message: None,
            expanded: false,
            meters: Vec::new(),
            fader: FaderState::default(),
        }
    }

    pub fn apply(&mut self, event: Event, now: f64) {
        match event {
            Event::Connected(identity) => {
                if !identity.serial.is_empty() {
                    self.key = SpeakerKey::Serial(identity.serial.clone());
                }
                self.identity = Some(identity);
                self.status = Status::Connected;
                self.message = None;
            }
            // The masked reason is for the log; the card only shows the generic text.
            Event::Unreachable(_) => {
                self.status = Status::Unreachable;
                self.message = Some(texts::UNREACHABLE.to_owned());
            }
            Event::Value { path, value } => {
                self.absent.remove(&path);
                self.feed_meters(&path, &value, now);
                self.values.insert(path, value);
            }
            Event::Absent(path) => {
                self.values.remove(&path);
                self.absent.insert(path);
            }
            Event::WriteRejected { error, .. } => {
                self.message = Some(texts::describe_rejection(error.code));
            }
            Event::WriteFailed { .. } => {
                self.message = Some(texts::WRITE_FAILED.to_owned());
            }
            Event::SerialMismatch { found, .. } => {
                self.status = Status::Unreachable;
                self.message = Some(texts::serial_mismatch(&mask_serial(&found)));
            }
            // The full-value dump has no state to land in until the report view exists.
            Event::AllValues(_) | Event::AllValuesFailed(_) => {}
        }
    }

    fn feed_meters(&mut self, path: &SscPath, value: &Value, now: f64) {
        let path_text = path.to_string();
        if !METER_PATHS.contains(&path_text.as_str()) {
            return;
        }
        let Some(levels) = value.as_array() else {
            return;
        };
        if self.meters.len() < levels.len() {
            self.meters.resize_with(levels.len(), MeterState::default);
        }
        for (meter, level) in self.meters.iter_mut().zip(levels) {
            meter.update(level.as_f64().unwrap_or(METER_FLOOR_DB), now);
        }
    }
}

/// Manual entries first (settings order), then discovered speakers not yet listed. A serial
/// present on both sides is one target at the discovered address; entries without serial are
/// identified by address. Manual entries whose address does not parse are skipped here and stay
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
    replace_address: bool,
) {
    match targets.iter_mut().find(|(existing, _)| *existing == key) {
        Some(existing) if replace_address => existing.1 = address,
        Some(_) => {}
        None => targets.push((key, address)),
    }
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

    pub fn peak(&self) -> f64 {
        self.peak
    }
}

/// Collects mouse-wheel steps so one write goes out after the wheel went quiet.
#[derive(Clone, Copy, Debug, Default)]
pub struct FaderState {
    pending: Option<f64>,
    last_step: f64,
}

impl FaderState {
    pub fn on_wheel(&mut self, steps: i32, current: f64, now: f64) {
        let base = self.pending.unwrap_or(current);
        let target = base + f64::from(steps) * WHEEL_STEP_DB;
        self.pending = Some(target.clamp(FADER_MIN_DB, FADER_MAX_DB));
        self.last_step = now;
    }

    pub fn due(&mut self, now: f64) -> Option<f64> {
        let idle = now - self.last_step >= WHEEL_IDLE_SECONDS;
        if self.pending.is_some() && idle {
            return self.pending.take();
        }
        None
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
        assert_eq!(speaker.meters.len(), 2);
        assert_eq!(speaker.meters[1].level(), -20.5);
    }

    #[test]
    fn peak_holds_then_falls_after_1_5_s() {
        let mut meter = MeterState::default();
        meter.update(-6.0, 0.0);
        meter.update(-30.0, 1.0);
        assert_eq!(meter.peak(), -6.0);
        assert_eq!(meter.level(), -30.0);
        meter.update(-30.0, 1.6);
        assert_eq!(meter.peak(), -30.0);
    }

    #[test]
    fn meter_fraction_clamps_below_floor_and_above_ceiling() {
        assert_eq!(fraction(-160.0), 0.0);
        assert_eq!(fraction(10.0), 1.0);
        assert_eq!(fraction(-27.0), 0.5);
    }

    #[test]
    fn wheel_steps_debounce_to_one_value() {
        let mut fader = FaderState::default();
        fader.on_wheel(1, 80.0, 1.0);
        fader.on_wheel(1, 80.0, 1.1);
        fader.on_wheel(1, 80.0, 1.2);
        assert_eq!(fader.shown(80.0), 81.5);
        assert_eq!(fader.due(1.4), None);
        assert_eq!(fader.due(1.5), Some(81.5));
        assert_eq!(fader.due(2.0), None);
        assert_eq!(fader.shown(80.0), 80.0);
    }

    #[test]
    fn wheel_clamps_at_120() {
        let mut fader = FaderState::default();
        fader.on_wheel(10, 119.0, 0.0);
        assert_eq!(fader.due(1.0), Some(120.0));
        fader.on_wheel(-1000, 1.0, 2.0);
        assert_eq!(fader.due(3.0), Some(0.0));
    }
}
