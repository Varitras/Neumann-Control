//! Masking of addresses and serial numbers for logs and screenshots.

use std::net::{Ipv6Addr, SocketAddrV6};

use crate::address::scope_suffix;

const MASK_PLACEHOLDER: &str = "xx";
const SEGMENT_COUNT: usize = 8;
const KEPT_PREFIX_SEGMENTS: usize = 4;
const LOW_BYTE_MASK: u16 = 0x00ff;
const KEPT_SERIAL_CHARACTERS: usize = 3;
const ELLIPSIS: char = '…';
/// Shorter digit runs are years, levels or model numbers rather than a serial.
const SERIAL_LIKE_DIGITS: usize = 8;
/// Stands for every value a report or a shown device text leaves out.
pub const REDACTED: &str = "<redacted>";
const MAC_GROUPS: usize = 6;
const MAC_GROUP_LENGTH: usize = 2;
/// A full IPv6 address has seven colons; a shortened one has `::`. A clock time has two at most.
const IPV6_FULL_COLONS: usize = 7;
const IPV6_SHORTENED: &str = "::";
const IPV4_GROUPS: usize = 4;
/// Punctuation around an address in a word; brackets are not among it, `[address]:port` is
/// told by them.
const WORD_TRIM: &str = ",;()<>\"'";
/// What may end a sentence right after an address ("reached at 192.0.2.5.").
const SENTENCE_END: [char; 4] = ['.', '!', '?', ':'];
const PORT_SEPARATOR: char = ':';

/// Keeps the first four groups (the network prefix) and only the low byte of
/// the last group, so the interface identifier never reaches logs.
pub fn mask_host(address: &SocketAddrV6) -> String {
    let segments = address.ip().segments();
    let mut prefix_segments = [0u16; SEGMENT_COUNT];
    prefix_segments[..KEPT_PREFIX_SEGMENTS].copy_from_slice(&segments[..KEPT_PREFIX_SEGMENTS]);
    let prefix = Ipv6Addr::from(prefix_segments);
    let last_byte = segments[SEGMENT_COUNT - 1] & LOW_BYTE_MASK;
    let scope = scope_suffix(address);
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

/// `text` with each word that holds an address replaced by `REDACTED`: an IPv4, IPv6 or MAC
/// address, also as `key=address`, `[address]:port`, `address:port`, `address/prefix` or followed
/// by sentence punctuation. The one rule for every way a device text leaves the app.
pub fn mask_addresses_in(text: &str) -> String {
    let mut masked = String::new();
    // Whitespace of any kind separates words; it stays as it was.
    for piece in text.split_inclusive(char::is_whitespace) {
        let word = piece.trim_end_matches(char::is_whitespace);
        masked.push_str(&masked_word(word));
        masked.push_str(&piece[word.len()..]);
    }
    masked
}

/// `mask_addresses_in`, then `mask_serial_like`: for a device text whose speaker is not known.
pub fn mask_device_text(text: &str) -> String {
    mask_serial_like(&mask_addresses_in(text))
}

fn masked_word(word: &str) -> String {
    let value = word.rsplit_once('=').map_or(word, |(_, value)| value);
    let bare = value.trim_matches(|character: char| WORD_TRIM.contains(character));
    let bracketed = bare
        .strip_prefix('[')
        .and_then(|rest| rest.split_once(']'))
        .map(|(inside, _)| inside);
    let candidate = bracketed.unwrap_or(bare);
    let candidate = candidate.split('/').next().unwrap_or_default();
    // "addr:fe80::1": a label of letters before the colon, then the address.
    let after_label = || {
        let (label, rest) = candidate.split_once(PORT_SEPARATOR)?;
        let is_label =
            !label.is_empty() && label.chars().all(|letter| letter.is_ascii_alphabetic());
        is_label.then(|| address_in(rest)).flatten()
    };
    match address_in(candidate).or_else(after_label) {
        Some(address) => word.replace(address, REDACTED),
        None => word.to_owned(),
    }
}

/// The address `candidate` is, without sentence punctuation after it or a port.
fn address_in(candidate: &str) -> Option<&str> {
    let trimmed = candidate.trim_end_matches(SENTENCE_END);
    let without_port = trimmed
        .rsplit_once(PORT_SEPARATOR)
        .filter(|(_, port)| !port.is_empty() && port.chars().all(|digit| digit.is_ascii_digit()))
        .map(|(host, _)| host)
        .filter(|host| looks_like_ipv4(host));
    [Some(candidate), Some(trimmed), without_port]
        .into_iter()
        .flatten()
        .find(|address| !address.is_empty() && looks_like_address(address))
}

fn looks_like_address(word: &str) -> bool {
    looks_like_mac(word) || looks_like_ipv6(word) || looks_like_ipv4(word)
}

fn looks_like_ipv4(word: &str) -> bool {
    let groups: Vec<&str> = word.split('.').collect();
    groups.len() == IPV4_GROUPS && groups.iter().all(|group| group.parse::<u8>().is_ok())
}

fn looks_like_mac(word: &str) -> bool {
    let groups: Vec<&str> = word.split([':', '-']).collect();
    groups.len() == MAC_GROUPS
        && groups.iter().all(|group| {
            group.len() == MAC_GROUP_LENGTH && group.chars().all(|digit| digit.is_ascii_hexdigit())
        })
}

fn looks_like_ipv6(word: &str) -> bool {
    let address = word.split('%').next().unwrap_or_default();
    let colons = address.matches(':').count();
    let allowed = address
        .chars()
        .all(|character| character.is_ascii_hexdigit() || character == ':' || character == '.');
    let has_digit = address
        .chars()
        .any(|character| character.is_ascii_hexdigit());
    let shaped = address.contains(IPV6_SHORTENED) || colons >= IPV6_FULL_COLONS;
    shaped && allowed && has_digit
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
        // From eight digits on: a seven-digit number stays.
        assert_eq!(mask_serial_like("KH 750 1234567"), "KH 750 1234567");
        assert_eq!(mask_serial_like("KH 750 12345678"), "KH 750 …678");
        assert_eq!(mask_serial_like("KH 120 II 2024"), "KH 120 II 2024");
    }

    /// An address after a label with a colon, or in angle brackets, is masked too: both stayed.
    #[test]
    fn an_address_after_a_label_or_in_angle_brackets_is_masked() {
        for text in [
            "addr:fe80::1 up",
            "ip:192.0.2.5",
            "peer <fe80::1> lost",
            "mac:00:11:22:33:44:55",
        ] {
            let masked = mask_addresses_in(text);
            for address in ["fe80::1", "192.0.2.5", "00:11:22:33:44:55"] {
                assert!(!masked.contains(address), "{text} -> {masked}");
            }
        }
        // A word that is no address keeps its colon.
        assert_eq!(mask_addresses_in("state:ok"), "state:ok");
    }
}
