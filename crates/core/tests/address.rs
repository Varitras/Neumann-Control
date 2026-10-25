use std::net::{Ipv6Addr, SocketAddrV6};

use neumann_control_core::address::{
    format_host, is_link_local, parse_address, AddressError, SSC_PORT,
};
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
        assert_eq!(address.port(), SSC_PORT);
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
fn host_mask_hides_interface_identifier_of_uncompressed_address() {
    let address = parse_address("2001:db8:1:2:2a36:38ff:fe12:3456", SSC_PORT).unwrap();
    let masked = mask_host(&address);
    assert_eq!(masked, "2001:db8:1:2::xx56");
    for hidden in ["2a36", "38ff", "fe12"] {
        assert!(!masked.contains(hidden));
    }
}

#[test]
fn scope_with_plus_sign_is_not_numeric() {
    assert_eq!(
        parse_address("fe80::1%+12", SSC_PORT),
        Err(AddressError::ScopeNotNumeric)
    );
}

#[test]
fn empty_scope_is_not_numeric() {
    assert_eq!(
        parse_address("fe80::1%", SSC_PORT),
        Err(AddressError::ScopeNotNumeric)
    );
}

#[test]
fn loopback_needs_no_scope() {
    let address = parse_address("::1", SSC_PORT).unwrap();
    assert_eq!(*address.ip(), Ipv6Addr::LOCALHOST);
    assert_eq!(address.scope_id(), 0);
}

#[test]
fn bracketed_address_keeps_its_scope() {
    let address = parse_address(" [fe80::1%12] ", SSC_PORT).unwrap();
    assert_eq!(address.scope_id(), 12);
    assert_eq!(format_host(&address), "fe80::1%12");
}

#[test]
fn ipv4_mapped_address_is_refused() {
    assert_eq!(
        parse_address("::ffff:192.168.1.10", SSC_PORT),
        Err(AddressError::Ipv4NotSupported)
    );
}

#[test]
fn serial_mask_keeps_last_three() {
    assert_eq!(mask_serial("SIM0001234"), "…234");
}

#[test]
fn short_serial_is_unchanged() {
    assert_eq!(mask_serial("123"), "123");
}

#[test]
fn a_formatted_host_parses_back_to_the_same_address() {
    let link_local = SocketAddrV6::new(SIMULATED_ADDRESS.parse().unwrap(), SSC_PORT, 0, 7);
    let global = SocketAddrV6::new("2001:db8::1".parse().unwrap(), SSC_PORT, 0, 0);
    assert_eq!(format_host(&link_local), format!("{SIMULATED_ADDRESS}%7"));
    assert_eq!(format_host(&global), "2001:db8::1");
    for address in [link_local, global] {
        assert_eq!(parse_address(&format_host(&address), SSC_PORT), Ok(address));
    }
}
