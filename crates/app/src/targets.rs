//! Which speakers to talk to: manual entries and scan results merged into one target each.

use std::net::SocketAddrV6;

use neumann_control_core::address::parse_address;
use neumann_control_core::discovery::Found;
use neumann_control_core::settings::ManualSpeaker;

use crate::state::SpeakerKey;

/// Manual entries first (settings order), then discovered speakers not yet listed. A speaker
/// present on both sides (same serial, or same address) is one target at the discovered
/// address, keyed by the serial once either side knows it. Manual entries whose address does
/// not parse are skipped here and stay untouched in the settings.
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

/// Writes the serial a speaker reported into the manual entries for its address that have none
/// yet, so the next merge joins them with scan results by serial. True when an entry changed.
pub fn remember_serial(manual: &mut [ManualSpeaker], serial: &str, address: SocketAddrV6) -> bool {
    if serial.is_empty() {
        return false;
    }
    let mut changed = false;
    for entry in manual.iter_mut() {
        let same_address = parse_address(&entry.address, entry.port) == Ok(address);
        if same_address && entry.serial.is_none() {
            entry.serial = Some(serial.to_owned());
            changed = true;
        }
    }
    changed
}

/// Positions of cards whose serial an earlier card already has: one speaker reached at two
/// addresses. The first card stays.
pub fn duplicate_cards(keys: &[SpeakerKey]) -> Vec<usize> {
    keys.iter()
        .enumerate()
        .filter(|(position, key)| {
            matches!(key, SpeakerKey::Serial(_)) && keys[..*position].contains(key)
        })
        .map(|(position, _)| position)
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_reported_serial_lands_in_manual_entries_without_one() {
        let mut entries = vec![
            manual(ADDRESS_TEXT, None),
            manual(OTHER_ADDRESS_TEXT, None),
            manual(ADDRESS_TEXT, Some(OTHER_SERIAL)),
        ];
        assert!(remember_serial(&mut entries, SERIAL, address(ADDRESS_TEXT)));
        assert_eq!(entries[0].serial.as_deref(), Some(SERIAL));
        assert_eq!(entries[1].serial, None);
        assert_eq!(entries[2].serial.as_deref(), Some(OTHER_SERIAL));
        assert!(!remember_serial(
            &mut entries,
            SERIAL,
            address(ADDRESS_TEXT)
        ));
        assert!(!remember_serial(
            &mut entries,
            "",
            address(OTHER_ADDRESS_TEXT)
        ));
    }

    #[test]
    fn later_cards_with_a_known_serial_are_duplicates() {
        let keys = [
            SpeakerKey::Address(address(ADDRESS_TEXT)),
            SpeakerKey::Serial(SERIAL.to_owned()),
            SpeakerKey::Address(address(ADDRESS_TEXT)),
            SpeakerKey::Serial(OTHER_SERIAL.to_owned()),
            SpeakerKey::Serial(SERIAL.to_owned()),
        ];
        assert_eq!(duplicate_cards(&keys), vec![4]);
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
}
