//! Which input channels a speaker plays, from its rear input switch (`ui/input_select`). The
//! input meters show channels A and B of the input stage; the switch decides what the speaker
//! plays of them. That A is the left AES3 channel is assumed, not yet measured.

use crate::paths;
use crate::state::SpeakerState;
use crate::texts;

const ANALOG: &str = "ANALOG";
const AES3_LEFT: &str = "AES3 L";
const AES3_RIGHT: &str = "AES3 R";
const MONO: &str = "MONO";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputSource {
    /// Shown under the input meters instead of the generic caption.
    pub caption: texts::Text,
    /// Whether channel A and channel B reach the speaker.
    pub played: [bool; 2],
}

/// None for models without the switch (KH 750) and for values the speakers never reported.
pub fn input_source(speaker: &SpeakerState) -> Option<InputSource> {
    let value = speaker.values.get(&paths::path(paths::INPUT_SELECT))?;
    let (caption, played) = match value.as_str()? {
        ANALOG => (texts::SOURCE_ANALOG, [true, true]),
        AES3_LEFT => (texts::SOURCE_AES3_LEFT, [true, false]),
        AES3_RIGHT => (texts::SOURCE_AES3_RIGHT, [false, true]),
        MONO => (texts::SOURCE_MONO, [true, true]),
        _ => return None,
    };
    Some(InputSource { caption, played })
}

#[cfg(test)]
mod tests {
    use super::*;
    use neumann_control_core::address::parse_address;
    use neumann_control_core::speaker::Event;
    use serde_json::{json, Value};

    use crate::state::SpeakerKey;

    fn reporting(select: Value) -> SpeakerState {
        let address = parse_address("fe80::2a36:38ff:fe12:3456%7", 45).unwrap();
        let mut speaker = SpeakerState::new(SpeakerKey::Address(address), address);
        let path = paths::path(paths::INPUT_SELECT);
        speaker.apply(
            Event::Value {
                path,
                value: select,
            },
            0.0,
        );
        speaker
    }

    #[test]
    fn the_switch_decides_which_channels_are_played() {
        let cases = [
            ("ANALOG", texts::SOURCE_ANALOG, [true, true]),
            ("AES3 L", texts::SOURCE_AES3_LEFT, [true, false]),
            ("AES3 R", texts::SOURCE_AES3_RIGHT, [false, true]),
            ("MONO", texts::SOURCE_MONO, [true, true]),
        ];
        for (reported, caption, played) in cases {
            let source = input_source(&reporting(json!(reported)));
            assert_eq!(source, Some(InputSource { caption, played }), "{reported}");
        }
    }

    #[test]
    fn unknown_or_missing_switch_gives_no_source() {
        assert_eq!(input_source(&reporting(json!("SOMETHING NEW"))), None);
        assert_eq!(input_source(&reporting(json!(3))), None);
    }
}
