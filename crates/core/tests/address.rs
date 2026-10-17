use std::net::{Ipv6Addr, SocketAddrV6};

use neumann_control_core::address::{is_link_local, parse_address, AddressError, SSC_PORT};
use neumann_control_core::mask::{mask_host, mask_serial};

const SIMULATED_ADDRESS: &str = "fe80::2a36:38ff:fe12:3456";

#[test]
fn link_local_without_scope_is_refused() {
    assert_eq!(
        parse_address(SIMULATED_ADDRESS, SSC_PORT),
        Err(AddressError::MissingScope)
    );
}

#[test]
fn interface_name_scope_is_refused() {
    assert_eq!(
        parse_address("fe80::1%eth0", SSC_PORT),
        Err(AddressError::ScopeNotNumeric)
    );
}

#[test]
fn ipv4_is_refused() {
    assert_eq!(
        parse_address("192.168.1.10", SSC_PORT),
        Err(AddressError::Ipv4NotSupported)
    );
}

#[test]
fn garbage_is_invalid() {
    assert_eq!(
        parse_address("speaker", SSC_PORT),
        Err(AddressError::Invalid)
    );
}

#[test]
fn bracketed_and_plain_forms_parse() {
    for input in ["fe80::1%12", "[fe80::1%12]", "  fe80::1%12  "] {
        let address = parse_address(input, SSC_PORT).unwrap();
        assert_eq!(address.scope_id(), 12);
        assert_eq!(address.port(), 45);
        assert_eq!(*address.ip(), "fe80::1".parse::<Ipv6Addr>().unwrap());
    }
}

#[test]
fn global_address_needs_no_scope() {
    let address = parse_address("2001:db8::1", SSC_PORT).unwrap();
    assert_eq!(address.scope_id(), 0);
}

#[test]
fn link_local_range_is_fe80_slash_10() {
    assert!(is_link_local(&"febf::1".parse().unwrap()));
    assert!(!is_link_local(&"fec0::1".parse().unwrap()));
}

#[test]
fn host_mask_keeps_prefix_and_last_two() {
    let address = parse_address("fe80::2a36:38ff:fe12:3456%12", SSC_PORT).unwrap();
    assert_eq!(mask_host(&address), "fe80::xx56%12");
}

#[test]
fn host_mask_without_scope_has_no_suffix() {
    let address = SocketAddrV6::new("2001:db8::abcd".parse().unwrap(), SSC_PORT, 0, 0);
    assert_eq!(mask_host(&address), "2001:db8::xxcd");
}

#[test]
fn host_mask_of_uncompressed_address_masks_final_group() {
    let address = SocketAddrV6::new("2001:db8:1:2:3:4:5:6789".parse().unwrap(), SSC_PORT, 0, 0);
    assert_eq!(mask_host(&address), "2001:db8:1:2:3:4:5:xx89");
}

#[test]
fn serial_mask_keeps_last_three() {
    assert_eq!(mask_serial("SIM0001234"), "…234");
}

#[test]
fn short_serial_is_unchanged() {
    assert_eq!(mask_serial("123"), "123");
}
