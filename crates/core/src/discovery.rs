//! Speaker discovery over mDNS. The speakers announce `_ssc._tcp.local.`, are reachable over IPv6
//! only, and need a link-local address together with the interface index as scope.

use std::net::{Ipv6Addr, SocketAddrV6};
use std::time::{Duration, Instant};

use mdns_sd::{IfKind, ScopedIp, ServiceDaemon, ServiceEvent};

use crate::address::is_link_local;
use crate::mask::mask_serial;

pub const SCAN_DURATION: Duration = Duration::from_secs(4);
pub const MAX_SERVICES: usize = 32;
const SERVICE_TYPE: &str = "_ssc._tcp.local.";
const MAX_TXT_CHARACTERS: usize = 64;
const TXT_PRODUCT_KEY: &str = "model";
const TXT_SERIAL_KEY: &str = "id";
const SERIAL_SEPARATOR: char = '-';
const NO_SCOPE: u32 = 0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// The announced instance name without its "-<serial>" suffix; any other occurrence of the
    /// serial is masked.
    pub name: String,
    pub product: Option<String>,
    pub serial: Option<String>,
    pub address: SocketAddrV6,
}

/// Link-local with a scope wins; a link-local address without a scope cannot be connected to on
/// Windows and is skipped; a global address is the fallback.
pub fn pick_address(addresses: &[(Ipv6Addr, u32)], port: u16) -> Option<SocketAddrV6> {
    let scoped_link_local = addresses
        .iter()
        .find(|(address, scope)| is_link_local(address) && *scope != NO_SCOPE);
    let global = addresses
        .iter()
        .find(|(address, _)| !is_link_local(address));
    scoped_link_local
        .or(global)
        .map(|(address, scope)| SocketAddrV6::new(*address, port, 0, *scope))
}

/// One entry per serial (a later link-local address replaces an earlier one, a later global
/// address never replaces a link-local one); entries without serial are deduplicated by address.
pub fn merge_found(found: Vec<Found>) -> Vec<Found> {
    let mut merged: Vec<Found> = Vec::new();
    for candidate in found {
        let existing = merged
            .iter_mut()
            .find(|entry| same_speaker(entry, &candidate));
        let Some(entry) = existing else {
            merged.push(candidate);
            continue;
        };
        let keeps_link_local =
            is_link_local(entry.address.ip()) && !is_link_local(candidate.address.ip());
        if !keeps_link_local {
            *entry = candidate;
        }
    }
    merged
}

fn same_speaker(left: &Found, right: &Found) -> bool {
    match (&left.serial, &right.serial) {
        (Some(left_serial), Some(right_serial)) => left_serial == right_serial,
        (None, None) => left.address == right.address,
        _ => false,
    }
}

fn instance_name<'a>(fullname: &'a str, service_type: &str) -> &'a str {
    let suffix = format!(".{service_type}");
    fullname.strip_suffix(&suffix).unwrap_or(fullname)
}

fn capped(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_control())
        .take(MAX_TXT_CHARACTERS)
        .collect()
}

/// A TXT value that is empty after cleaning counts as missing, so two malformed speakers are
/// never merged under the same empty serial.
fn txt_value(text: Option<&str>) -> Option<String> {
    text.map(capped).filter(|value| !value.is_empty())
}

/// Adds one resolved speaker; mdns-sd re-emits resolutions per interface and refresh, so the
/// speaker cap has to count the merged list, not the events.
fn add_found(mut found: Vec<Found>, candidate: Found) -> Vec<Found> {
    found.push(candidate);
    merge_found(found)
}

/// Speakers announce "<device name>-<serial>"; the suffix only repeats the TXT serial. A name
/// that is nothing but the suffix is kept, so the speaker never loses its announced name.
fn without_serial_suffix<'a>(name: &'a str, serial: &str) -> &'a str {
    let suffix = format!("{SERIAL_SEPARATOR}{serial}");
    name.strip_suffix(&suffix)
        .filter(|rest| !rest.is_empty())
        .unwrap_or(name)
}

fn found_from_parts(
    fullname: &str,
    service_type: &str,
    port: u16,
    addresses: &[(Ipv6Addr, u32)],
    product: Option<&str>,
    serial: Option<&str>,
) -> Option<Found> {
    let serial = txt_value(serial);
    let mut name = instance_name(fullname, service_type).to_owned();
    if let Some(serial) = &serial {
        name = without_serial_suffix(&name, serial).replace(serial.as_str(), &mask_serial(serial));
    }
    Some(Found {
        name: capped(&name),
        product: txt_value(product),
        serial,
        address: pick_address(addresses, port)?,
    })
}

/// Browses for `duration` and returns every speaker that resolved with a usable address. It may
/// return early once `MAX_SERVICES` distinct speakers are found.
/// Errors are fixed texts: the underlying messages could carry network addresses.
pub fn scan(duration: Duration) -> Result<Vec<Found>, String> {
    let daemon = ServiceDaemon::new().map_err(|_| "mDNS could not be started".to_string())?;
    let result = browse(&daemon, duration);
    // The scan is over either way; a failed shutdown leaves nothing to recover.
    let _ = daemon.shutdown();
    result
}

fn browse(daemon: &ServiceDaemon, duration: Duration) -> Result<Vec<Found>, String> {
    daemon
        .disable_interface(IfKind::IPv4)
        .map_err(|_| "mDNS could not be restricted to IPv6".to_string())?;
    let events = daemon
        .browse(SERVICE_TYPE)
        .map_err(|_| "mDNS browsing could not be started".to_string())?;
    let deadline = Instant::now() + duration;
    let mut found: Vec<Found> = Vec::new();
    while found.len() < MAX_SERVICES {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let Ok(event) = events.recv_timeout(remaining) else {
            break;
        };
        let ServiceEvent::ServiceResolved(service) = event else {
            continue;
        };
        let addresses: Vec<(Ipv6Addr, u32)> = service
            .get_addresses()
            .iter()
            .filter_map(|address| match address {
                ScopedIp::V6(scoped) => Some((*scoped.addr(), scoped.scope_id().index)),
                _ => None,
            })
            .collect();
        let entry = found_from_parts(
            service.get_fullname(),
            &service.ty_domain,
            service.get_port(),
            &addresses,
            service.get_property_val_str(TXT_PRODUCT_KEY),
            service.get_property_val_str(TXT_SERIAL_KEY),
        );
        if let Some(entry) = entry {
            found = add_found(found, entry);
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_name_loses_the_service_suffix() {
        assert_eq!(
            instance_name("KH 120 II 1._ssc._tcp.local.", SERVICE_TYPE),
            "KH 120 II 1"
        );
        assert_eq!(instance_name("plain", SERVICE_TYPE), "plain");
    }

    #[test]
    fn txt_values_are_capped() {
        let long = "x".repeat(MAX_TXT_CHARACTERS + 10);
        let address: Ipv6Addr = "2001:db8::1".parse().unwrap();
        let found = found_from_parts(
            "a._ssc._tcp.local.",
            SERVICE_TYPE,
            45,
            &[(address, 0)],
            Some(&long),
            Some(&long),
        )
        .unwrap();
        assert_eq!(found.product.unwrap().chars().count(), MAX_TXT_CHARACTERS);
        assert_eq!(found.serial.unwrap().chars().count(), MAX_TXT_CHARACTERS);
    }

    fn sample(serial: Option<&str>, last_segment: u16) -> Found {
        let address = Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, last_segment);
        Found {
            name: "Speaker".into(),
            product: None,
            serial: serial.map(String::from),
            address: SocketAddrV6::new(address, 45, 0, 0),
        }
    }

    #[test]
    fn repeated_events_do_not_fill_the_cap() {
        let mut found = Vec::new();
        for _ in 0..MAX_SERVICES + 5 {
            found = add_found(found, sample(Some("SIM0001234"), 1));
        }
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn empty_serial_is_treated_as_missing() {
        let address: Ipv6Addr = "2001:db8::1".parse().unwrap();
        let found = found_from_parts(
            "a._ssc._tcp.local.",
            SERVICE_TYPE,
            45,
            &[(address, 0)],
            Some(""),
            Some(""),
        )
        .unwrap();
        assert_eq!(found.serial, None);
        assert_eq!(found.product, None);
    }

    #[test]
    fn the_serial_in_the_instance_name_is_masked() {
        let address: Ipv6Addr = "2001:db8::1".parse().unwrap();
        let found = found_from_parts(
            "KH 120 II SIM0001234._ssc._tcp.local.",
            SERVICE_TYPE,
            45,
            &[(address, 0)],
            Some("KH 120 II"),
            Some("SIM0001234"),
        )
        .unwrap();
        assert!(!found.name.contains("SIM0001234"), "{}", found.name);
        assert_eq!(
            found.name,
            format!("KH 120 II {}", mask_serial("SIM0001234"))
        );
        assert_eq!(found.serial.as_deref(), Some("SIM0001234"));
    }

    fn announced_name(fullname: &str, serial: Option<&str>) -> String {
        let address: Ipv6Addr = "2001:db8::1".parse().unwrap();
        found_from_parts(
            fullname,
            SERVICE_TYPE,
            45,
            &[(address, 0)],
            Some("KH 120 II"),
            serial,
        )
        .unwrap()
        .name
    }

    #[test]
    fn a_trailing_dash_serial_is_stripped_from_the_instance_name() {
        assert_eq!(
            announced_name("Desk left-SIM0001234._ssc._tcp.local.", Some("SIM0001234")),
            "Desk left"
        );
    }

    #[test]
    fn a_serial_elsewhere_in_the_instance_name_is_masked() {
        assert_eq!(
            announced_name(
                "SIM0001234 desk-SIM0001234._ssc._tcp.local.",
                Some("SIM0001234")
            ),
            format!("{} desk", mask_serial("SIM0001234"))
        );
    }

    #[test]
    fn without_a_txt_serial_the_instance_name_stays_unchanged() {
        assert_eq!(
            announced_name("Desk left-SIM0001234._ssc._tcp.local.", None),
            "Desk left-SIM0001234"
        );
    }

    #[test]
    fn an_instance_name_that_is_only_the_suffix_keeps_its_masked_serial() {
        let name = announced_name("-SIM0001234._ssc._tcp.local.", Some("SIM0001234"));
        assert_eq!(name, format!("-{}", mask_serial("SIM0001234")));
    }

    #[test]
    fn control_characters_are_removed_from_txt() {
        assert_eq!(capped("KH\u{0}1\n20\u{1b}"), "KH120");
    }

    #[test]
    fn service_without_usable_address_is_dropped() {
        let link_local: Ipv6Addr = "fe80::1".parse().unwrap();
        let found = found_from_parts(
            "a._ssc._tcp.local.",
            SERVICE_TYPE,
            45,
            &[(link_local, 0)],
            None,
            None,
        );
        assert_eq!(found, None);
    }
}
