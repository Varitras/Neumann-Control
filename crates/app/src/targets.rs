//! Which speakers to talk to: the remembered entries in the settings, one target each, and how
//! scan results relate to them.

use std::net::SocketAddrV6;

use neumann_control_core::address::{format_host, parse_address};
use neumann_control_core::discovery::Found;
use neumann_control_core::settings::ManualSpeaker;

use crate::state::SpeakerKey;

/// One target per remembered entry in settings order, keyed by the serial once it is known.
/// A later entry with a key or address already listed is the same speaker and is skipped, as
/// are entries whose address does not parse; both stay untouched in the settings.
pub fn targets(manual: &[ManualSpeaker]) -> Vec<(SpeakerKey, SocketAddrV6)> {
    let mut targets: Vec<(SpeakerKey, SocketAddrV6)> = Vec::new();
    for speaker in manual {
        let Ok(address) = parse_address(&speaker.address, speaker.port) else {
            continue;
        };
        let key = match &speaker.serial {
            Some(serial) => SpeakerKey::Serial(serial.clone()),
            None => SpeakerKey::Address(address),
        };
        let listed = targets
            .iter()
            .any(|(existing, existing_address)| *existing == key || *existing_address == address);
        if !listed {
            targets.push((key, address));
        }
    }
    targets
}

/// Whether a scanned speaker already has a card: by serial when the scan reported one, by
/// address otherwise.
pub fn has_card(found: &Found, cards: &[(SpeakerKey, SocketAddrV6)]) -> bool {
    cards.iter().any(|(key, address)| match &found.serial {
        Some(serial) => *key == SpeakerKey::Serial(serial.clone()) || *address == found.address,
        None => *address == found.address,
    })
}

/// The settings entry that brings a scanned speaker back at the next start without a scan.
pub fn entry_for(found: &Found) -> ManualSpeaker {
    ManualSpeaker {
        address: format_host(&found.address),
        port: found.address.port(),
        serial: found.serial.clone(),
    }
}

/// Points remembered entries at the address a scan found their serial at: link-local
/// addresses and interface indexes can change between starts. An entry that then equals an
/// earlier one is dropped. True when an entry changed.
pub fn follow_moves(manual: &mut Vec<ManualSpeaker>, found: &[Found]) -> bool {
    let mut changed = false;
    for entry in manual.iter_mut() {
        let Some(serial) = &entry.serial else {
            continue;
        };
        let moved = found.iter().find(|candidate| {
            candidate.serial.as_ref() == Some(serial)
                && parse_address(&entry.address, entry.port) != Ok(candidate.address)
        });
        if let Some(moved) = moved {
            *entry = entry_for(moved);
            changed = true;
        }
    }
    if changed {
        let mut kept: Vec<ManualSpeaker> = Vec::new();
        for entry in manual.drain(..) {
            if !kept.contains(&entry) {
                kept.push(entry);
            }
        }
        *manual = kept;
    }
    changed
}

/// Drops every remembered entry of a removed card: those with its serial and those at its
/// address. True when an entry was removed.
pub fn forget(manual: &mut Vec<ManualSpeaker>, key: &SpeakerKey, address: SocketAddrV6) -> bool {
    let before = manual.len();
    manual.retain(|entry| {
        let same_serial = match key {
            SpeakerKey::Serial(serial) => entry.serial.as_ref() == Some(serial),
            SpeakerKey::Address(_) => false,
        };
        let same_address = parse_address(&entry.address, entry.port) == Ok(address);
        !(same_serial || same_address)
    });
    manual.len() != before
}

/// Whether a manual entry already points at `address`, however it was written.
pub fn is_listed(manual: &[ManualSpeaker], address: SocketAddrV6) -> bool {
    manual
        .iter()
        .any(|entry| parse_address(&entry.address, entry.port) == Ok(address))
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

#[cfg(test)]
mod tests {
    use super::*;

    const SERIAL: &str = "SIM0001234";
    const OTHER_SERIAL: &str = "SIM0007500";
    const ADDRESS_TEXT: &str = "fe80::2a36:38ff:fe12:3456%7";
    const OTHER_ADDRESS_TEXT: &str = "fe80::2a36:38ff:fe12:3457%7";
    const UNUSED_ADDRESS_TEXT: &str = "fe80::2a36:38ff:fe12:3458%7";
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
    fn each_remembered_speaker_is_one_target_in_settings_order() {
        let targets = targets(&[
            manual(ADDRESS_TEXT, None),
            manual("not an address", None),
            manual(OTHER_ADDRESS_TEXT, Some(SERIAL)),
            manual(ADDRESS_TEXT, Some(OTHER_SERIAL)),
            manual(ADDRESS_TEXT, None),
        ]);
        assert_eq!(
            targets,
            vec![
                (
                    SpeakerKey::Address(address(ADDRESS_TEXT)),
                    address(ADDRESS_TEXT)
                ),
                (
                    SpeakerKey::Serial(SERIAL.to_owned()),
                    address(OTHER_ADDRESS_TEXT)
                ),
            ]
        );
    }

    #[test]
    fn a_scanned_speaker_has_a_card_by_serial_or_else_by_address() {
        let cards = [
            (SpeakerKey::Serial(SERIAL.to_owned()), address(ADDRESS_TEXT)),
            (
                SpeakerKey::Address(address(OTHER_ADDRESS_TEXT)),
                address(OTHER_ADDRESS_TEXT),
            ),
        ];
        assert!(has_card(&found(UNUSED_ADDRESS_TEXT, Some(SERIAL)), &cards));
        assert!(has_card(&found(OTHER_ADDRESS_TEXT, None), &cards));
        assert!(has_card(
            &found(OTHER_ADDRESS_TEXT, Some(OTHER_SERIAL)),
            &cards
        ));
        assert!(!has_card(
            &found(UNUSED_ADDRESS_TEXT, Some(OTHER_SERIAL)),
            &cards
        ));
        assert!(!has_card(&found(UNUSED_ADDRESS_TEXT, None), &cards));
    }

    #[test]
    fn the_entry_for_a_scanned_speaker_parses_back_to_its_address() {
        let entry = entry_for(&found(ADDRESS_TEXT, Some(SERIAL)));
        assert_eq!(entry, manual(ADDRESS_TEXT, Some(SERIAL)));
        assert_eq!(
            parse_address(&entry.address, entry.port),
            Ok(address(ADDRESS_TEXT))
        );
    }

    #[test]
    fn a_saved_serial_found_elsewhere_moves_its_entry() {
        let mut entries = vec![
            manual(ADDRESS_TEXT, Some(SERIAL)),
            manual(OTHER_ADDRESS_TEXT, None),
            manual(UNUSED_ADDRESS_TEXT, Some(OTHER_SERIAL)),
        ];
        let scanned = [
            found(OTHER_ADDRESS_TEXT, Some(SERIAL)),
            found(UNUSED_ADDRESS_TEXT, Some(OTHER_SERIAL)),
        ];
        assert!(follow_moves(&mut entries, &scanned));
        assert_eq!(
            entries,
            vec![
                manual(OTHER_ADDRESS_TEXT, Some(SERIAL)),
                manual(OTHER_ADDRESS_TEXT, None),
                manual(UNUSED_ADDRESS_TEXT, Some(OTHER_SERIAL)),
            ]
        );
        assert!(!follow_moves(&mut entries, &scanned));
    }

    #[test]
    fn an_entry_moved_onto_an_equal_one_is_dropped() {
        let mut entries = vec![
            manual(ADDRESS_TEXT, Some(SERIAL)),
            manual(OTHER_ADDRESS_TEXT, Some(SERIAL)),
        ];
        assert!(follow_moves(
            &mut entries,
            &[found(OTHER_ADDRESS_TEXT, Some(SERIAL))]
        ));
        assert_eq!(entries, vec![manual(OTHER_ADDRESS_TEXT, Some(SERIAL))]);
    }

    #[test]
    fn forgetting_a_card_drops_its_serial_and_its_address() {
        let mut entries = vec![
            manual(ADDRESS_TEXT, None),
            manual(OTHER_ADDRESS_TEXT, Some(SERIAL)),
            manual(UNUSED_ADDRESS_TEXT, Some(OTHER_SERIAL)),
        ];
        let key = SpeakerKey::Serial(SERIAL.to_owned());
        assert!(forget(&mut entries, &key, address(ADDRESS_TEXT)));
        assert_eq!(
            entries,
            vec![manual(UNUSED_ADDRESS_TEXT, Some(OTHER_SERIAL))]
        );
        let unknown = SpeakerKey::Address(address(ADDRESS_TEXT));
        assert!(!forget(&mut entries, &unknown, address(ADDRESS_TEXT)));
    }
}
