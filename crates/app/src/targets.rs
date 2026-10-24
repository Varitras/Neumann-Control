//! Which speakers to talk to: the remembered entries in the settings, one target each, and how
//! scan results relate to them.

use std::collections::BTreeMap;
use std::net::SocketAddrV6;

use neumann_control_core::address::{format_host, parse_address};
use neumann_control_core::discovery::Found;
use neumann_control_core::settings::{KnownSpeaker, ManualSpeaker};
use neumann_control_core::speaker::Identity;

use crate::state::SpeakerKey;

/// One target per remembered entry in settings order, keyed by the serial once it is known.
/// A later entry of a speaker already listed (`same_speaker`) is skipped, as are entries whose
/// address does not parse; both stay untouched in the settings.
pub fn targets(manual: &[ManualSpeaker]) -> Vec<(SpeakerKey, SocketAddrV6)> {
    let mut targets: Vec<(SpeakerKey, SocketAddrV6)> = Vec::new();
    for (key, address) in manual.iter().filter_map(target_of) {
        let listed = targets.iter().any(|(existing, existing_address)| {
            same_speaker(existing, *existing_address, &key, address)
        });
        if !listed {
            targets.push((key, address));
        }
    }
    targets
}

fn target_of(entry: &ManualSpeaker) -> Option<(SpeakerKey, SocketAddrV6)> {
    let address = parse_address(&entry.address, entry.port).ok()?;
    let key = match &entry.serial {
        Some(serial) => SpeakerKey::Serial(serial.clone()),
        None => SpeakerKey::Address(address),
    };
    Some((key, address))
}

/// The same key, or the same address while one of the two has no serial yet. Two serials at
/// one address are two speakers: one moved there after the other left.
pub fn same_speaker(
    key: &SpeakerKey,
    address: SocketAddrV6,
    other_key: &SpeakerKey,
    other_address: SocketAddrV6,
) -> bool {
    let by_address = [key, other_key]
        .iter()
        .any(|key| matches!(key, SpeakerKey::Address(_)));
    key == other_key || (by_address && address == other_address)
}

/// Whether a scanned speaker is already shown (`same_speaker`): by serial, and by address where
/// one of the two has none.
pub fn is_shown(found: &Found, shown: &[(SpeakerKey, SocketAddrV6)]) -> bool {
    let found_key = match &found.serial {
        Some(serial) => SpeakerKey::Serial(serial.clone()),
        None => SpeakerKey::Address(found.address),
    };
    shown
        .iter()
        .any(|(key, address)| same_speaker(key, *address, &found_key, found.address))
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

/// Drops every remembered entry of a removed speaker (`same_speaker`); another speaker's entry
/// at its address stays. True when an entry was removed.
pub fn forget(manual: &mut Vec<ManualSpeaker>, key: &SpeakerKey, address: SocketAddrV6) -> bool {
    let before = manual.len();
    manual.retain(|entry| {
        let target = target_of(entry);
        !target.is_some_and(|(entry_key, entry_address)| {
            same_speaker(&entry_key, entry_address, key, address)
        })
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

/// Keeps a speaker's name and product under its serial, so it is named while it is off; true
/// when that changed what is kept.
pub fn remember_identity(known: &mut BTreeMap<String, KnownSpeaker>, identity: &Identity) -> bool {
    if identity.serial.is_empty() {
        return false;
    }
    let kept = KnownSpeaker {
        name: identity.name.clone(),
        product: identity.product.clone(),
    };
    known.insert(identity.serial.clone(), kept.clone()) != Some(kept)
}

/// Positions of speakers whose serial an earlier one already has: one speaker reached at two
/// addresses. The first stays.
pub fn duplicate_speakers(keys: &[SpeakerKey]) -> Vec<usize> {
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

    fn identity(serial: &str, name: &str) -> Identity {
        Identity {
            product: "KH 750".to_owned(),
            version: "2_1_2".to_owned(),
            serial: serial.to_owned(),
            name: name.to_owned(),
        }
    }

    #[test]
    fn an_identity_is_kept_under_its_serial_and_changes_count_once() {
        let mut known = BTreeMap::new();
        assert!(remember_identity(&mut known, &identity("SIM1", "Sub")));
        assert!(!remember_identity(&mut known, &identity("SIM1", "Sub")));
        assert!(remember_identity(&mut known, &identity("SIM1", "Sub left")));
        assert_eq!(known["SIM1"].name, "Sub left");
        assert!(!remember_identity(&mut known, &identity("", "Nobody")));
        assert_eq!(known.len(), 1);
    }
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

    /// A scan found S where T was remembered (T is off, the address went to S): both stay
    /// targets, S at its new address and T where it was last, told apart by their serials.
    #[test]
    fn two_serials_at_one_address_are_two_targets() {
        let mut entries = vec![
            manual(ADDRESS_TEXT, Some(OTHER_SERIAL)),
            manual(OTHER_ADDRESS_TEXT, Some(SERIAL)),
        ];
        assert!(follow_moves(
            &mut entries,
            &[found(ADDRESS_TEXT, Some(SERIAL))]
        ));
        let targets = targets(&entries);
        assert!(targets.contains(&(SpeakerKey::Serial(SERIAL.to_owned()), address(ADDRESS_TEXT))));
        assert!(targets.contains(&(
            SpeakerKey::Serial(OTHER_SERIAL.to_owned()),
            address(ADDRESS_TEXT)
        )));
    }

    /// Removing T, which was remembered where S is now, leaves S remembered.
    #[test]
    fn forgetting_a_speaker_keeps_another_serial_at_its_address() {
        let mut entries = vec![
            manual(ADDRESS_TEXT, Some(OTHER_SERIAL)),
            manual(ADDRESS_TEXT, Some(SERIAL)),
            manual(ADDRESS_TEXT, None),
        ];
        let key = SpeakerKey::Serial(OTHER_SERIAL.to_owned());
        assert!(forget(&mut entries, &key, address(ADDRESS_TEXT)));
        assert_eq!(entries, vec![manual(ADDRESS_TEXT, Some(SERIAL))]);
    }

    #[test]
    fn later_speakers_with_a_known_serial_are_duplicates() {
        let keys = [
            SpeakerKey::Address(address(ADDRESS_TEXT)),
            SpeakerKey::Serial(SERIAL.to_owned()),
            SpeakerKey::Address(address(ADDRESS_TEXT)),
            SpeakerKey::Serial(OTHER_SERIAL.to_owned()),
            SpeakerKey::Serial(SERIAL.to_owned()),
        ];
        assert_eq!(duplicate_speakers(&keys), vec![4]);
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
    fn a_scanned_speaker_is_shown_by_serial_or_else_by_address() {
        let shown = [
            (SpeakerKey::Serial(SERIAL.to_owned()), address(ADDRESS_TEXT)),
            (
                SpeakerKey::Address(address(OTHER_ADDRESS_TEXT)),
                address(OTHER_ADDRESS_TEXT),
            ),
        ];
        assert!(is_shown(&found(UNUSED_ADDRESS_TEXT, Some(SERIAL)), &shown));
        assert!(is_shown(&found(OTHER_ADDRESS_TEXT, None), &shown));
        assert!(is_shown(
            &found(OTHER_ADDRESS_TEXT, Some(OTHER_SERIAL)),
            &shown
        ));
        assert!(!is_shown(
            &found(UNUSED_ADDRESS_TEXT, Some(OTHER_SERIAL)),
            &shown
        ));
        assert!(!is_shown(&found(UNUSED_ADDRESS_TEXT, None), &shown));
        // Another serial where a speaker with a serial is shown: that one moved away.
        assert!(!is_shown(&found(ADDRESS_TEXT, Some(OTHER_SERIAL)), &shown));
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
    fn forgetting_a_speaker_drops_its_serial_and_its_address() {
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
