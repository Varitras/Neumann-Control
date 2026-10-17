//! Speaker discovery over mDNS. The speakers announce `_ssc._tcp.local.`, are reachable over IPv6
//! only, and need a link-local address together with the interface index as scope.

use std::net::{Ipv6Addr, SocketAddrV6};
use std::time::{Duration, Instant};

use mdns_sd::{IfKind, ScopedIp, ServiceDaemon, ServiceEvent};

use crate::address::is_link_local;

pub const SCAN_DURATION: Duration = Duration::from_secs(4);
pub const MAX_SERVICES: usize = 32;
const SERVICE_TYPE: &str = "_ssc._tcp.local.";
const MAX_TXT_CHARACTERS: usize = 64;
const TXT_PRODUCT_KEY: &str = "model";
const TXT_SERIAL_KEY: &str = "id";
const NO_SCOPE: u32 = 0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
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
    text.chars().take(MAX_TXT_CHARACTERS).collect()
}

fn found_from_parts(
    fullname: &str,
    service_type: &str,
    port: u16,
    addresses: &[(Ipv6Addr, u32)],
    product: Option<&str>,
    serial: Option<&str>,
) -> Option<Found> {
    Some(Found {
        name: capped(instance_name(fullname, service_type)),
        product: product.map(capped),
        serial: serial.map(capped),
        address: pick_address(addresses, port)?,
    })
}

/// Browses for `duration` and returns every speaker that resolved with a usable address.
/// Errors are fixed texts: the underlying messages could carry network addresses.
pub fn scan(duration: Duration) -> Result<Vec<Found>, String> {
    let daemon = ServiceDaemon::new().map_err(|_| "mDNS could not be started".to_string())?;
    let result = browse(&daemon, duration);
    // The scan is over either way; a failed shutdown leaves nothing to recover.
    let _ = daemon.shutdown();
    result.map(merge_found)
}

fn browse(daemon: &ServiceDaemon, duration: Duration) -> Result<Vec<Found>, String> {
    daemon
        .disable_interface(IfKind::IPv4)
        .map_err(|_| "mDNS could not be restricted to IPv6".to_string())?;
    let events = daemon
        .browse(SERVICE_TYPE)
        .map_err(|_| "mDNS browsing could not be started".to_string())?;
    let deadline = Instant::now() + duration;
    let mut found = Vec::new();
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
        found.extend(entry);
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
