use std::net::{Ipv6Addr, SocketAddrV6};

use neumann_control_core::discovery::{merge_found, pick_address, Found};

const PORT: u16 = 45;
const SIMULATED_LINK_LOCAL: &str = "fe80::2a36:38ff:fe12:3456";
const SIMULATED_GLOBAL: &str = "2001:db8::2a36:38ff:fe12:3456";

fn address(text: &str) -> Ipv6Addr {
    text.parse().unwrap()
}

fn found(serial: Option<&str>, text: &str, scope: u32) -> Found {
    Found {
        name: "Speaker".into(),
        product: Some("KH 120 II".into()),
        serial: serial.map(String::from),
        address: SocketAddrV6::new(address(text), PORT, 0, scope),
    }
}

#[test]
fn link_local_with_scope_is_preferred() {
    let addresses = [
        (address(SIMULATED_GLOBAL), 0),
        (address(SIMULATED_LINK_LOCAL), 7),
    ];
    let picked = pick_address(&addresses, PORT).unwrap();
    assert_eq!(*picked.ip(), address(SIMULATED_LINK_LOCAL));
    assert_eq!(picked.scope_id(), 7);
    assert_eq!(picked.port(), PORT);
}

#[test]
fn link_local_without_scope_is_ignored() {
    let addresses = [(address(SIMULATED_LINK_LOCAL), 0)];
    assert_eq!(pick_address(&addresses, PORT), None);
}

#[test]
fn global_is_used_when_no_link_local() {
    let addresses = [
        (address(SIMULATED_LINK_LOCAL), 0),
        (address(SIMULATED_GLOBAL), 0),
    ];
    let picked = pick_address(&addresses, PORT).unwrap();
    assert_eq!(*picked.ip(), address(SIMULATED_GLOBAL));
    assert_eq!(picked.scope_id(), 0);
}

#[test]
fn same_serial_is_merged() {
    let merged = merge_found(vec![
        found(Some("SIM0001234"), SIMULATED_GLOBAL, 0),
        found(Some("SIM0001234"), SIMULATED_LINK_LOCAL, 7),
        found(Some("SIM0007500"), SIMULATED_GLOBAL, 0),
    ]);
    assert_eq!(merged.len(), 2);
    assert_eq!(*merged[0].address.ip(), address(SIMULATED_LINK_LOCAL));
    assert_eq!(merged[0].address.scope_id(), 7);
}

#[test]
fn link_local_is_kept_when_a_global_comes_later() {
    let merged = merge_found(vec![
        found(Some("SIM0001234"), SIMULATED_LINK_LOCAL, 7),
        found(Some("SIM0001234"), SIMULATED_GLOBAL, 0),
    ]);
    assert_eq!(merged.len(), 1);
    assert_eq!(*merged[0].address.ip(), address(SIMULATED_LINK_LOCAL));
}

#[test]
fn entries_without_serial_are_kept_by_address() {
    let merged = merge_found(vec![
        found(None, SIMULATED_LINK_LOCAL, 7),
        found(None, SIMULATED_LINK_LOCAL, 7),
        found(None, SIMULATED_GLOBAL, 0),
    ]);
    let addresses: Vec<Ipv6Addr> = merged.iter().map(|entry| *entry.address.ip()).collect();
    assert_eq!(
        addresses,
        [address(SIMULATED_LINK_LOCAL), address(SIMULATED_GLOBAL)]
    );
}

#[test]
fn a_later_link_local_address_replaces_an_earlier_one() {
    let merged = merge_found(vec![
        found(Some("SIM0001234"), SIMULATED_LINK_LOCAL, 7),
        found(Some("SIM0001234"), SIMULATED_LINK_LOCAL, 15),
    ]);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].address.scope_id(), 15);
}
