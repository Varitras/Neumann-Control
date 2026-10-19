//! The write policy: what the user asked for becomes a `Command` only for a connected speaker
//! whose model the catalog lists as writable for the path, with a value the catalog accepts.
//! Anything else is refused: `request` puts the reason on the speaker's message line, and
//! `try_request` hands it to the caller (a text field shows it under itself).

use neumann_control_core::catalog::{Catalog, Entry, ValueError};
use neumann_control_core::speaker::Command;
use neumann_control_core::ssc::wire::SscPath;
use serde_json::Value;

use crate::paths;
use crate::state::{Message, MessageSource, SpeakerState, Status};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Switch {
    Mute,
    Identify,
}

impl Switch {
    pub fn path(self) -> SscPath {
        match self {
            Switch::Mute => paths::path(paths::MUTE),
            Switch::Identify => paths::path(paths::IDENTIFY),
        }
    }
}

/// What a control of a speaker asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum Intent {
    /// Flip a switch to the opposite of its current value.
    Toggle(Switch),
    /// Set the output level.
    Level(f64),
    /// Set any catalog path to the given value.
    Write { path: SscPath, value: Value },
}

impl Intent {
    pub fn path(&self) -> SscPath {
        match self {
            Intent::Toggle(switch) => switch.path(),
            Intent::Level(_) => paths::path(paths::OUTPUT_LEVEL),
            Intent::Write { path, .. } => path.clone(),
        }
    }
}

/// The value to write for a number the user entered: on the entry's step grid, and whole
/// when the speaker reports the path as a whole number, so the write has the device's own type.
pub fn number_value(entry: &Entry, reported: Option<&Value>, number: f64) -> Value {
    let snapped = match entry.step.filter(|step| *step > 0.0) {
        Some(step) => {
            let origin = entry.min.unwrap_or(0.0);
            origin + ((number - origin) / step).round() * step
        }
        None => number,
    };
    let whole = reported.is_some_and(|value| value.is_i64() || value.is_u64());
    if whole && snapped.fract() == 0.0 {
        return Value::from(snapped as i64);
    }
    Value::from(snapped)
}

#[derive(Clone, Debug, PartialEq)]
pub enum Refusal {
    NotConnected,
    NotWritable,
    /// A switch whose current value the speaker has not reported yet.
    ValueUnknown,
    Invalid(ValueError),
}

/// Whether a control for `path` may be offered at all.
pub fn can_write(speaker: &SpeakerState, catalog: &Catalog, path: &SscPath) -> bool {
    let product = speaker.product();
    let writable =
        paths::catalog_entry(catalog, path).is_some_and(|entry| entry.is_writable(product));
    speaker.status == Status::Connected && writable
}

/// The checks of the policy, in the order the user would fix them.
pub fn check(
    speaker: &SpeakerState,
    catalog: &Catalog,
    path: &SscPath,
    value: &Value,
) -> Result<(), Refusal> {
    if speaker.status != Status::Connected {
        return Err(Refusal::NotConnected);
    }
    let product = speaker.product();
    let entry = paths::catalog_entry(catalog, path)
        .filter(|entry| entry.is_writable(product))
        .ok_or(Refusal::NotWritable)?;
    entry
        .validate(value, product, &catalog.classes)
        .map_err(Refusal::Invalid)
}

impl SpeakerState {
    /// The command for `intent`, or `None` after putting the refusal on the speaker's message.
    /// A sent level is held on the fader until the speaker reports it (`now` is the UI clock).
    pub fn request(&mut self, intent: Intent, catalog: &Catalog, now: f64) -> Option<Command> {
        match self.try_request(intent, catalog, now) {
            Ok(command) => Some(command),
            Err(refusal) => {
                self.set_message(Some(Message::Refused(refusal)), MessageSource::Write);
                None
            }
        }
    }

    /// Like `request`, but the refusal goes back to the caller instead of onto the message.
    pub fn try_request(
        &mut self,
        intent: Intent,
        catalog: &Catalog,
        now: f64,
    ) -> Result<Command, Refusal> {
        self.clear_write_message();
        let path = intent.path();
        let checked = self
            .intended_value(&intent, &path)
            .and_then(|value| check(self, catalog, &path, &value).map(|()| value));
        let value = checked.inspect_err(|refusal| log::warn!("not sending {path}: {refusal:?}"))?;
        // A sent value stays on its control until the speaker reports it; a refused one does not.
        if let Intent::Level(level) = intent {
            self.fader.sent(level, now);
        }
        if let (Some(knob), Some(number)) = (self.knobs.get_mut(&path), value.as_f64()) {
            knob.sent(number, now);
        }
        self.record_written(&path, &value);
        Ok(Command::Set { path, value })
    }

    fn intended_value(&self, intent: &Intent, path: &SscPath) -> Result<Value, Refusal> {
        match intent {
            Intent::Level(level) => Ok(Value::from(*level)),
            Intent::Write { value, .. } => Ok(value.clone()),
            Intent::Toggle(_) => {
                let current = self.values.get(path).and_then(Value::as_bool);
                let current = current.ok_or(Refusal::ValueUnknown)?;
                Ok(Value::Bool(!current))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neumann_control_core::address::parse_address;
    use neumann_control_core::discovery::Found;
    use neumann_control_core::speaker::{Event, Identity};

    use crate::state::SpeakerKey;

    const ADDRESS_TEXT: &str = "fe80::2a36:38ff:fe12:3456%7";

    fn speaker(product: &str) -> SpeakerState {
        let address = parse_address(ADDRESS_TEXT, 45).unwrap();
        let mut speaker = SpeakerState::new(SpeakerKey::Address(address), address);
        let identity = Identity {
            product: product.to_owned(),
            version: "1".to_owned(),
            serial: "SIM0001234".to_owned(),
            name: "left".to_owned(),
        };
        speaker.apply(Event::Connected(identity), 0.0);
        speaker.apply(
            Event::Value {
                path: paths::path(paths::MUTE),
                value: Value::Bool(false),
            },
            0.0,
        );
        speaker
    }

    fn set(path: &str, value: Value) -> Option<Command> {
        Some(Command::Set {
            path: paths::path(path),
            value,
        })
    }

    #[test]
    fn toggle_writes_the_opposite_switch_value() {
        let mut speaker = speaker("KH 120 II");
        let command = speaker.request(Intent::Toggle(Switch::Mute), Catalog::embedded(), 0.0);
        assert_eq!(command, set(paths::MUTE, Value::Bool(true)));
        assert_eq!(speaker.message, None);
    }

    #[test]
    fn a_sent_level_is_held_on_the_fader() {
        let mut speaker = speaker("KH 120 II");
        let command = speaker.request(Intent::Level(90.0), Catalog::embedded(), 0.0);
        assert_eq!(command, set(paths::OUTPUT_LEVEL, Value::from(90.0)));
        assert_eq!(speaker.fader.shown(80.0, 0.5), 90.0);
    }

    #[test]
    fn a_model_without_write_permission_is_refused_with_a_message() {
        let mut speaker = speaker("KH 80");
        let command = speaker.request(Intent::Level(80.0), Catalog::embedded(), 0.0);
        assert_eq!(command, None);
        let expected = Message::Refused(Refusal::NotWritable);
        assert_eq!(speaker.message, Some(expected));
        assert!(!can_write(
            &speaker,
            Catalog::embedded(),
            &paths::path(paths::OUTPUT_LEVEL)
        ));
    }

    #[test]
    fn an_out_of_range_value_is_refused_with_the_catalog_reason() {
        let mut speaker = speaker("KH 120 II");
        let command = speaker.request(Intent::Level(130.0), Catalog::embedded(), 0.0);
        assert_eq!(command, None);
        let reason = ValueError::OutOfRange {
            min: 0.0,
            max: 120.0,
        };
        let expected = Message::Refused(Refusal::Invalid(reason));
        assert_eq!(speaker.message, Some(expected));
    }

    #[test]
    fn an_unreachable_speaker_is_refused() {
        let mut speaker = speaker("KH 120 II");
        speaker.apply(Event::Unreachable("masked".to_owned()), 1.0);
        assert_eq!(
            check(
                &speaker,
                Catalog::embedded(),
                &paths::path(paths::MUTE),
                &Value::Bool(true)
            ),
            Err(Refusal::NotConnected)
        );
        assert!(!can_write(
            &speaker,
            Catalog::embedded(),
            &paths::path(paths::MUTE)
        ));
    }

    fn entry(path: &str) -> &'static Entry {
        paths::catalog_entry(Catalog::embedded(), &paths::path(path)).unwrap()
    }

    #[test]
    fn a_write_intent_sends_the_given_value() {
        let mut speaker = speaker("KH 120 II");
        let intent = Intent::Write {
            path: paths::path("device/standby/enabled"),
            value: Value::Bool(false),
        };
        let command = speaker.request(intent, Catalog::embedded(), 0.0);
        assert_eq!(command, set("device/standby/enabled", Value::Bool(false)));
    }

    #[test]
    fn a_write_intent_outside_the_catalog_rules_is_refused() {
        let mut speaker = speaker("KH 120 II");
        let intent = Intent::Write {
            path: paths::path("device/name"),
            value: Value::from("Küche"),
        };
        assert_eq!(speaker.request(intent, Catalog::embedded(), 0.0), None);
        let expected = Message::Refused(Refusal::Invalid(ValueError::NotPrintableAscii));
        assert_eq!(speaker.message, Some(expected));
    }

    #[test]
    fn a_refusal_stays_through_polls_until_the_next_request() {
        let mut speaker = speaker("KH 120 II");
        speaker.request(Intent::Level(130.0), Catalog::embedded(), 0.0);
        let level = Event::Value {
            path: paths::path(paths::OUTPUT_LEVEL),
            value: Value::from(80.0),
        };
        speaker.apply(level, 2.0);
        assert!(speaker.message.is_some(), "a poll ended the refusal");
        let command = speaker.request(Intent::Toggle(Switch::Mute), Catalog::embedded(), 3.0);
        assert_eq!(command, set(paths::MUTE, Value::Bool(true)));
        assert_eq!(speaker.message, None);
    }

    #[test]
    fn a_request_keeps_the_unreachable_message() {
        let mut speaker = speaker("KH 120 II");
        speaker.apply(Event::Unreachable("masked".to_owned()), 1.0);
        let intent = Intent::Write {
            path: paths::path("device/name"),
            value: Value::from("left"),
        };
        assert!(speaker
            .try_request(intent, Catalog::embedded(), 2.0)
            .is_err());
        assert_eq!(speaker.message, Some(Message::Unreachable));
    }

    #[test]
    fn the_announced_model_does_not_allow_writes() {
        let mut speaker = speaker("");
        speaker.discovered = Some(Found {
            name: String::new(),
            product: Some("KH 120 II".to_owned()),
            serial: None,
            address: parse_address(ADDRESS_TEXT, 45).unwrap(),
        });
        let level = paths::path(paths::OUTPUT_LEVEL);
        assert!(!can_write(&speaker, Catalog::embedded(), &level));
        assert_eq!(
            check(&speaker, Catalog::embedded(), &level, &Value::from(80.0)),
            Err(Refusal::NotWritable)
        );
    }

    #[test]
    fn a_number_snaps_to_the_step_and_keeps_the_reported_type() {
        let delay = entry("audio/out/delay");
        assert_eq!(
            number_value(delay, Some(&Value::from(0)), 12.4),
            Value::from(12)
        );
        assert_eq!(
            number_value(delay, Some(&Value::from(0.0)), 12.6),
            Value::from(13.0)
        );
        let standby_level = entry("device/standby/level");
        assert_eq!(
            number_value(standby_level, Some(&Value::from(-65)), -70.2),
            Value::from(-70)
        );
        let level = entry(paths::OUTPUT_LEVEL);
        assert_eq!(
            number_value(level, Some(&Value::from(100.0)), 90.25),
            Value::from(90.25)
        );
    }

    #[test]
    fn a_sent_write_is_recorded_with_the_value_before() {
        let mut speaker = speaker("KH 120 II");
        let level = Event::Value {
            path: paths::path(paths::OUTPUT_LEVEL),
            value: Value::from(80.0),
        };
        speaker.apply(level, 0.0);
        speaker.request(Intent::Level(90.0), Catalog::embedded(), 0.0);
        // The speaker has not reported 90 yet: the next write started from 90, not 80.
        speaker.request(Intent::Level(95.0), Catalog::embedded(), 0.1);
        let path = paths::path(paths::OUTPUT_LEVEL);
        let written = std::mem::take(&mut speaker.written);
        assert_eq!(
            written,
            vec![
                (path.clone(), Some(Value::from(80.0)), Value::from(90.0)),
                (path.clone(), Some(Value::from(90.0)), Value::from(95.0)),
            ]
        );
        let reported = Event::Value {
            path: path.clone(),
            value: Value::from(93.0),
        };
        speaker.apply(reported, 0.5);
        speaker.request(Intent::Level(85.0), Catalog::embedded(), 1.0);
        assert_eq!(speaker.written[0].1, Some(Value::from(93.0)));
    }

    #[test]
    fn identify_and_refused_writes_are_not_recorded() {
        let mut speaker = speaker("KH 120 II");
        let identify = Event::Value {
            path: paths::path(paths::IDENTIFY),
            value: Value::Bool(false),
        };
        speaker.apply(identify, 0.0);
        speaker.request(Intent::Toggle(Switch::Identify), Catalog::embedded(), 0.0);
        speaker.request(Intent::Level(130.0), Catalog::embedded(), 0.0);
        assert!(speaker.written.is_empty(), "{:?}", speaker.written);
    }

    #[test]
    fn the_kh_750_writes_standby_time_and_level_but_not_auto_standby() {
        // Measured on a KH 750 (firmware 2_1_2): time and level take a write and read back;
        // auto standby follows the rear AUTO STANDBY switch.
        let speaker = speaker("KH 750");
        let catalog = Catalog::embedded();
        assert!(can_write(
            &speaker,
            catalog,
            &paths::path(paths::STANDBY_TIME)
        ));
        assert!(can_write(
            &speaker,
            catalog,
            &paths::path("device/standby/level")
        ));
        assert!(!can_write(
            &speaker,
            catalog,
            &paths::path("device/standby/enabled")
        ));
    }

    #[test]
    fn a_second_band_write_keeps_the_first_pending() {
        let mut speaker = speaker("KH 120 II");
        let path = paths::path("audio/out/eq2/gain");
        speaker.apply(
            Event::Value {
                path: path.clone(),
                value: serde_json::json!([0.0, 0.0, 0.0]),
            },
            0.0,
        );
        let write = |value| Intent::Write {
            path: path.clone(),
            value,
        };
        let catalog = Catalog::embedded();
        assert!(speaker
            .request(write(serde_json::json!([3.0, null, null])), catalog, 0.0)
            .is_some());
        assert!(speaker
            .request(write(serde_json::json!([null, 4.0, null])), catalog, 0.1)
            .is_some());
        assert!(speaker
            .request(write(serde_json::json!([5.0, null, null])), catalog, 0.2)
            .is_some());
        let befores: Vec<Option<Value>> = speaker
            .written
            .iter()
            .map(|change| change.1.clone())
            .collect();
        assert_eq!(
            befores,
            vec![
                Some(serde_json::json!([0.0, null, null])),
                Some(serde_json::json!([null, 0.0, null])),
                Some(serde_json::json!([3.0, null, null])),
            ]
        );
    }

    #[test]
    fn a_switch_without_a_reported_value_is_refused() {
        let mut speaker = speaker("KH 120 II");
        let command = speaker.request(Intent::Toggle(Switch::Identify), Catalog::embedded(), 0.0);
        assert_eq!(command, None);
        let expected = Message::Refused(Refusal::ValueUnknown);
        assert_eq!(speaker.message, Some(expected));
    }
}
