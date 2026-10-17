//! The write policy: what the user asked for becomes a `Command` only for a connected speaker
//! whose model the catalog lists as writable for the path, with a value the catalog accepts.
//! Anything else is refused with a message on the card.

use neumann_control_core::catalog::{Catalog, Entry, ValueError};
use neumann_control_core::speaker::Command;
use neumann_control_core::ssc::wire::SscPath;
use serde_json::Value;

use crate::paths;
use crate::state::{SpeakerState, Status};
use crate::texts;

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

/// The control mode in which the speaker may stop listening to the network.
const LOCAL_CONTROL_MODE: &str = "LOCAL";

/// What a control on the card asks for.
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

    /// Whether the user has to confirm before the write goes out: in LOCAL control mode the
    /// speaker may ignore the network until someone resets it at the device.
    pub fn needs_confirmation(&self) -> bool {
        let Intent::Write { path, value } = self else {
            return false;
        };
        *path == paths::path(paths::CONTROL_MODE) && value.as_str() == Some(LOCAL_CONTROL_MODE)
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
    let product = speaker.model().unwrap_or_default();
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
    let product = speaker.model().unwrap_or_default();
    let entry = paths::catalog_entry(catalog, path)
        .filter(|entry| entry.is_writable(product))
        .ok_or(Refusal::NotWritable)?;
    entry
        .validate(value, product, &catalog.classes)
        .map_err(Refusal::Invalid)
}

impl SpeakerState {
    /// The command for `intent`, or `None` after putting the refusal on the card.
    /// A sent level is held on the fader until the speaker reports it (`now` is the UI clock).
    pub fn request(&mut self, intent: Intent, catalog: &Catalog, now: f64) -> Option<Command> {
        let path = intent.path();
        let checked = self
            .intended_value(&intent, &path)
            .and_then(|value| check(self, catalog, &path, &value).map(|()| value));
        match checked {
            Ok(value) => {
                if let Intent::Level(level) = intent {
                    self.fader.sent(level, now);
                }
                Some(Command::Set { path, value })
            }
            Err(refusal) => {
                log::warn!("not sending {path}: {refusal:?}");
                self.set_message(Some(texts::describe_refusal(&refusal)), Some(path));
                None
            }
        }
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
        let expected = texts::describe_refusal(&Refusal::NotWritable);
        assert_eq!(speaker.message.as_deref(), Some(expected.as_str()));
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
        let expected = texts::describe_refusal(&Refusal::Invalid(reason));
        assert_eq!(speaker.message.as_deref(), Some(expected.as_str()));
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
        let expected = texts::describe_refusal(&Refusal::Invalid(ValueError::NotPrintableAscii));
        assert_eq!(speaker.message.as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn only_switching_to_local_control_needs_confirmation() {
        let write = |value: &str| Intent::Write {
            path: paths::path(paths::CONTROL_MODE),
            value: Value::from(value),
        };
        assert!(write("LOCAL").needs_confirmation());
        assert!(!write("NETWORK").needs_confirmation());
        assert!(!Intent::Level(80.0).needs_confirmation());
        let other = Intent::Write {
            path: paths::path("audio/in/interface"),
            value: Value::from("LOCAL"),
        };
        assert!(!other.needs_confirmation());
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
    fn a_switch_without_a_reported_value_is_refused() {
        let mut speaker = speaker("KH 120 II");
        let command = speaker.request(Intent::Toggle(Switch::Identify), Catalog::embedded(), 0.0);
        assert_eq!(command, None);
        let expected = texts::describe_refusal(&Refusal::ValueUnknown);
        assert_eq!(speaker.message.as_deref(), Some(expected.as_str()));
    }
}
