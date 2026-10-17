//! Masking of addresses and serial numbers for logs and screenshots.

use std::net::{Ipv6Addr, SocketAddrV6};

const MASK_PLACEHOLDER: &str = "xx";
const SEGMENT_COUNT: usize = 8;
const KEPT_PREFIX_SEGMENTS: usize = 4;
const LOW_BYTE_MASK: u16 = 0x00ff;
const KEPT_SERIAL_CHARACTERS: usize = 3;
const ELLIPSIS: char = '…';

/// Keeps the first four groups (the network prefix) and only the low byte of
/// the last group, so the interface identifier never reaches logs.
pub fn mask_host(address: &SocketAddrV6) -> String {
    let segments = address.ip().segments();
    let mut prefix_segments = [0u16; SEGMENT_COUNT];
    prefix_segments[..KEPT_PREFIX_SEGMENTS].copy_from_slice(&segments[..KEPT_PREFIX_SEGMENTS]);
    let prefix = Ipv6Addr::from(prefix_segments);
    let last_byte = segments[SEGMENT_COUNT - 1] & LOW_BYTE_MASK;
    let scope = match address.scope_id() {
        0 => String::new(),
        scope_id => format!("%{scope_id}"),
    };
    format!("{prefix}{MASK_PLACEHOLDER}{last_byte:02x}{scope}")
}

pub fn mask_serial(serial: &str) -> String {
    let characters: Vec<char> = serial.chars().collect();
    if characters.len() <= KEPT_SERIAL_CHARACTERS {
        return serial.to_owned();
    }
    let kept: String = characters[characters.len() - KEPT_SERIAL_CHARACTERS..]
        .iter()
        .collect();
    format!("{ELLIPSIS}{kept}")
}
