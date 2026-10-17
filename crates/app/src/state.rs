//! UI-independent state of one speaker card: what the worker events add up to.
//! All times are seconds of the UI clock passed in by the caller.

use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddrV6;

use neumann_control_core::discovery::Found;
use neumann_control_core::mask::{mask_host, mask_serial};
use neumann_control_core::schema_walk::LeafReport;
use neumann_control_core::speaker::{Command, Event, Identity};
use neumann_control_core::ssc::wire::SscPath;
use serde_json::Value;

use crate::fader::FaderState;
use crate::meter::{MeterState, METER_FLOOR_DB};
use crate::paths;
use crate::texts;

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

/// The answer to "Alle Werte…": every value the speaker has, read on request.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum AllValuesState {
    #[default]
    Idle,
    Loading,
    Loaded(Vec<LeafReport>),
    Failed(String),
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
    pub all_values: AllValuesState,
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
            all_values: AllValuesState::Idle,
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
                if path == paths::path(paths::OUTPUT_LEVEL) {
                    self.fader.confirmed();
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
            Event::AllValues(reports) => self.all_values = AllValuesState::Loaded(reports),
            Event::AllValuesFailed(reason) => self.all_values = AllValuesState::Failed(reason),
        }
    }

    /// Starts reading every value; the worker answers with `AllValues` or `AllValuesFailed`.
    pub fn read_all_values(&mut self) -> Command {
        self.all_values = AllValuesState::Loading;
        Command::ReadAll
    }

    pub(crate) fn set_message(&mut self, message: Option<String>, path: Option<SscPath>) {
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
            paths::INPUT_METER => &mut self.input_meters,
            paths::OUTPUT_METER => &mut self.output_meters,
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

#[cfg(test)]
mod tests {
    use super::*;
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
    fn a_reported_level_ends_the_held_fader_value() {
        let mut speaker = state();
        speaker.fader.sent(90.0, 0.0);
        speaker.apply(value_event("device/name", json!("left")), 0.5);
        assert_eq!(speaker.fader.shown(80.0, 0.5), 90.0);
        speaker.apply(value_event(paths::OUTPUT_LEVEL, json!(80.0)), 0.5);
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
}
