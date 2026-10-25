//! The four LEDs of a channel strip and the response time over all speakers, derived from
//! what the speakers reported. Standby has no LED: a speaker in standby does not answer and
//! shows as unreachable.

use std::time::Duration;

use serde_json::Value;

use crate::paths;
use crate::state::{SpeakerState, Status};

/// What `warnings` holds while the speaker has nothing to report.
const NO_WARNING: &str = "NO_WARNING";
const LOCAL_CONTROL_MODE: &str = "LOCAL";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionLed {
    Connected,
    Connecting,
    Off,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusLeds {
    pub connection: ConnectionLed,
    pub clip: bool,
    pub warning: bool,
    /// The rear switch is on LOCAL; the speaker refuses every write.
    pub local: bool,
}

pub fn status_leds(speaker: &SpeakerState) -> StatusLeds {
    let connection = match speaker.status {
        Status::Connected => ConnectionLed::Connected,
        Status::Connecting => ConnectionLed::Connecting,
        Status::Unreachable => ConnectionLed::Off,
    };
    // Without a connection the last report is history, not the speaker's state now.
    let connected = speaker.status == Status::Connected;
    let reported = |path: &str| speaker.values.get(&paths::path(path)).filter(|_| connected);
    let clip = [paths::INPUT_CLIP, paths::OUTPUT_CLIP]
        .into_iter()
        .filter_map(reported)
        .any(any_true);
    let warning = reported(paths::WARNINGS).is_some_and(holds_a_warning);
    let local = reported(paths::CONTROL_MODE).and_then(Value::as_str) == Some(LOCAL_CONTROL_MODE);
    StatusLeds {
        connection,
        clip,
        warning,
        local,
    }
}

/// The mean of the connected speakers' response times; none while no speaker has one.
pub fn average_response_time<'a>(
    speakers: impl Iterator<Item = &'a SpeakerState>,
) -> Option<Duration> {
    let times: Vec<Duration> = speakers
        .filter(|speaker| speaker.status == Status::Connected)
        .filter_map(|speaker| speaker.response_time)
        .collect();
    let count = u32::try_from(times.len()).ok().filter(|count| *count > 0)?;
    Some(times.iter().sum::<Duration>() / count)
}

fn any_true(value: &Value) -> bool {
    match value {
        Value::Bool(flag) => *flag,
        Value::Array(items) => items.iter().any(any_true),
        _ => false,
    }
}

/// The warnings `warnings` reports, without `NO_WARNING`: a text or a list of texts, entries of
/// another form passed over; `None` for a value that is neither. The LED and the trend both
/// read it through here.
pub fn active_warnings(value: &Value) -> Option<Vec<String>> {
    let texts: Vec<&str> = match value {
        Value::String(text) => vec![text.as_str()],
        Value::Array(items) => items.iter().filter_map(Value::as_str).collect(),
        _ => return None,
    };
    let active = texts.into_iter().filter(|text| *text != NO_WARNING);
    Some(active.map(str::to_owned).collect())
}

fn holds_a_warning(value: &Value) -> bool {
    active_warnings(value).is_some_and(|active| !active.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use neumann_control_core::address::parse_address;
    use neumann_control_core::speaker::{Event, Identity};
    use neumann_control_core::ssc::wire::SscPath;
    use serde_json::json;

    use crate::state::SpeakerKey;

    const ADDRESS_TEXT: &str = "fe80::2a36:38ff:fe12:3456%7";
    const PORT: u16 = neumann_control_core::address::SSC_PORT;

    fn connected() -> SpeakerState {
        let address = parse_address(ADDRESS_TEXT, PORT).unwrap();
        let mut speaker = SpeakerState::new(SpeakerKey::Address(address), address);
        speaker.apply(
            Event::Connected(Identity {
                product: "KH 120 II".to_owned(),
                version: "1_7_3".to_owned(),
                serial: "SIM0001234".to_owned(),
                name: "Front L".to_owned(),
            }),
            0.0,
        );
        speaker
    }

    /// What a speaker reported before it went away says nothing about it now: clip, warning
    /// and LOCAL stay dark; the connection LED tells the state.
    #[test]
    fn an_unreachable_speaker_shows_no_old_device_state() {
        let mut speaker = connected();
        report(&mut speaker, paths::INPUT_CLIP, json!([true, false]));
        report(&mut speaker, paths::WARNINGS, json!(["OVERTEMPERATURE"]));
        report(&mut speaker, paths::CONTROL_MODE, json!(LOCAL_CONTROL_MODE));
        speaker.status = Status::Unreachable;
        let leds = status_leds(&speaker);
        assert_eq!(leds.connection, ConnectionLed::Off);
        assert!(!leds.clip && !leds.warning && !leds.local, "{leds:?}");
    }

    fn report(speaker: &mut SpeakerState, path: &str, value: Value) {
        let path = SscPath::parse(path).unwrap();
        speaker.apply(Event::Value { path, value }, 0.0);
    }

    /// The warning LED and the warning trend read one value the same way, also an odd one.
    #[test]
    fn the_led_and_the_trend_agree_on_warnings() {
        let odd = json!(["OVERTEMP", 3]);
        assert_eq!(active_warnings(&odd), Some(vec!["OVERTEMP".to_owned()]));
        assert!(holds_a_warning(&odd));
        let calm = json!([NO_WARNING]);
        assert_eq!(active_warnings(&calm), Some(Vec::new()));
        assert!(!holds_a_warning(&calm));
        assert_eq!(
            active_warnings(&json!("OVERTEMP")),
            Some(vec!["OVERTEMP".to_owned()])
        );
        assert_eq!(active_warnings(&json!(7)), None);
        assert!(!holds_a_warning(&json!(7)));
    }

    #[test]
    fn leds_follow_connection_clip_warning_and_local() {
        let mut speaker = connected();
        report(&mut speaker, paths::INPUT_CLIP, json!([false, false]));
        report(&mut speaker, paths::WARNINGS, json!(["NO_WARNING"]));
        report(&mut speaker, paths::CONTROL_MODE, json!("NETWORK"));
        let quiet = status_leds(&speaker);
        assert_eq!(quiet.connection, ConnectionLed::Connected);
        assert!(!quiet.clip && !quiet.warning && !quiet.local);

        report(
            &mut speaker,
            paths::OUTPUT_CLIP,
            json!([false, true, false]),
        );
        report(&mut speaker, paths::WARNINGS, json!(["OVERTEMPERATURE"]));
        report(&mut speaker, paths::CONTROL_MODE, json!("LOCAL"));
        let lit = status_leds(&speaker);
        assert!(lit.clip && lit.warning && lit.local);

        speaker.apply(Event::Unreachable("gone".to_owned()), 1.0);
        assert_eq!(status_leds(&speaker).connection, ConnectionLed::Off);
    }

    /// An input that clips lights the LED alone, without an output clipping.
    #[test]
    fn an_input_clip_lights_the_clip_led() {
        let mut speaker = connected();
        report(
            &mut speaker,
            paths::OUTPUT_CLIP,
            json!([false, false, false]),
        );
        report(&mut speaker, paths::INPUT_CLIP, json!([false, true]));
        assert!(status_leds(&speaker).clip);
    }

    #[test]
    fn average_ignores_unconnected_speakers() {
        let mut first = connected();
        first.apply(Event::ResponseTime(Duration::from_millis(10)), 0.0);
        let mut second = connected();
        second.apply(Event::ResponseTime(Duration::from_millis(30)), 0.0);
        let mut gone = connected();
        gone.apply(Event::ResponseTime(Duration::from_millis(500)), 0.0);
        gone.status = Status::Unreachable;
        let speakers = [first, second, gone];
        assert_eq!(
            average_response_time(speakers.iter()),
            Some(Duration::from_millis(20))
        );
        assert_eq!(average_response_time([].iter()), None);
    }
}
