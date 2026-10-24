//! Masking of addresses and serial numbers for logs and screenshots.

use std::net::{Ipv6Addr, SocketAddrV6};

const MASK_PLACEHOLDER: &str = "xx";
const SEGMENT_COUNT: usize = 8;
const KEPT_PREFIX_SEGMENTS: usize = 4;
const LOW_BYTE_MASK: u16 = 0x00ff;
const KEPT_SERIAL_CHARACTERS: usize = 3;
const ELLIPSIS: char = '…';
/// Shorter digit runs are years, levels or model numbers rather than a serial.
const SERIAL_LIKE_DIGITS: usize = 8;

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

/// Keeps the last three characters, enough to tell two speakers apart in a log.
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

/// `text` with every occurrence of `serial` masked, in any case; a name such as "KH 120 II
/// <serial>" then tells two speakers apart without showing the serial.
pub fn mask_serial_in(text: &str, serial: &str) -> String {
    // An empty serial would be found at every position without moving on.
    if serial.is_empty() {
        return text.to_owned();
    }
    let lower_text = text.to_ascii_lowercase();
    let lower_serial = serial.to_ascii_lowercase();
    let mask = mask_serial(serial);
    let mut masked = String::new();
    let mut rest = 0;
    while let Some(offset) = lower_text[rest..].find(&lower_serial) {
        masked.push_str(&text[rest..rest + offset]);
        masked.push_str(&mask);
        rest += offset + serial.len();
    }
    masked.push_str(&text[rest..]);
    masked
}

/// For a text whose speaker's serial is not known: every run of at least
/// `SERIAL_LIKE_DIGITS` digits is masked as a serial (the speakers' serials are ten digits).
pub fn mask_serial_like(text: &str) -> String {
    let mut masked = String::new();
    let mut digits = String::new();
    let flush = |digits: &mut String, masked: &mut String| {
        match digits.chars().count() >= SERIAL_LIKE_DIGITS {
            true => masked.push_str(&mask_serial(digits)),
            false => masked.push_str(digits),
        }
        digits.clear();
    };
    for character in text.chars() {
        if character.is_ascii_digit() {
            digits.push(character);
            continue;
        }
        flush(&mut digits, &mut masked);
        masked.push(character);
    }
    flush(&mut digits, &mut masked);
    masked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_serial_inside_a_name_is_masked_wherever_it_stands() {
        assert_eq!(
            mask_serial_in("KH 120 II 3712345678 left", "3712345678"),
            "KH 120 II …678 left"
        );
        assert_eq!(mask_serial_in("Front", "3712345678"), "Front");
        assert_eq!(mask_serial_in("Front", ""), "Front");
    }

    /// Without a known serial, a run of digits as long as a serial is masked all the same.
    #[test]
    fn serial_like_digits_are_masked_when_the_serial_is_unknown() {
        assert_eq!(mask_serial_like("KH 750 3712345678"), "KH 750 …678");
        assert_eq!(mask_serial_like("KH 120 II 2024"), "KH 120 II 2024");
    }
}
