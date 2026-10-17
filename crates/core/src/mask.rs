//! Masking of addresses and serial numbers for logs and screenshots.

use std::net::SocketAddrV6;

const MASK_PLACEHOLDER: &str = "xx";
const KEPT_ADDRESS_CHARACTERS: usize = 2;
const KEPT_SERIAL_CHARACTERS: usize = 3;
const ELLIPSIS: char = '…';
const GROUP_SEPARATOR: char = ':';
const COMPRESSION_MARKER: &str = "::";

/// Keeps the prefix up to the last `::` and drops the groups after it, keeping
/// only the final two characters. Without a `::` only the last group is masked.
pub fn mask_host(address: &SocketAddrV6) -> String {
    let text = address.ip().to_string();
    let prefix_length = match text.rfind(COMPRESSION_MARKER) {
        Some(index) => index + COMPRESSION_MARKER.len(),
        None => text.rfind(GROUP_SEPARATOR).map_or(0, |index| index + 1),
    };
    let (prefix, tail) = text.split_at(prefix_length);
    let kept_start = tail.len().saturating_sub(KEPT_ADDRESS_CHARACTERS);
    let scope = match address.scope_id() {
        0 => String::new(),
        scope_id => format!("%{scope_id}"),
    };
    format!("{prefix}{MASK_PLACEHOLDER}{}{scope}", &tail[kept_start..])
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
