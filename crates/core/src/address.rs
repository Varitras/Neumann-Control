//! Parsing of IPv6 speaker addresses, including Windows numeric zone indexes.

use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddrV6};

/// The TCP port Sennheiser Sound Control listens on.
pub const SSC_PORT: u16 = 45;

const SCOPE_SEPARATOR: char = '%';
const LINK_LOCAL_PREFIX_MASK: u16 = 0xffc0;
const LINK_LOCAL_PREFIX: u16 = 0xfe80;
const NO_SCOPE: u32 = 0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AddressError {
    Ipv4NotSupported,
    MissingScope,
    ScopeNotNumeric,
    Invalid,
}

impl fmt::Display for AddressError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            AddressError::Ipv4NotSupported => "IPv4 addresses are not supported",
            AddressError::MissingScope => "link-local address needs an interface index scope",
            AddressError::ScopeNotNumeric => "scope must be a numeric interface index",
            AddressError::Invalid => "not a valid IPv6 address",
        };
        formatter.write_str(text)
    }
}

impl std::error::Error for AddressError {}

/// `fe80::/10`: reachable only through one interface, so it needs a scope.
pub fn is_link_local(address: &Ipv6Addr) -> bool {
    address.segments()[0] & LINK_LOCAL_PREFIX_MASK == LINK_LOCAL_PREFIX
}

/// Reads an address as typed or stored: optional brackets and blanks, a numeric `%index`
/// scope (required for link-local), no IPv4 or IPv4-mapped form.
pub fn parse_address(input: &str, port: u16) -> Result<SocketAddrV6, AddressError> {
    let trimmed = input.trim();
    let unbracketed = trimmed
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(trimmed);
    let (host, scope_text) = match unbracketed.split_once(SCOPE_SEPARATOR) {
        Some((host, scope_text)) => (host, Some(scope_text)),
        None => (unbracketed, None),
    };

    if host.parse::<Ipv4Addr>().is_ok() {
        return Err(AddressError::Ipv4NotSupported);
    }
    let ip: Ipv6Addr = host.parse().map_err(|_| AddressError::Invalid)?;
    if ip.to_ipv4_mapped().is_some() {
        return Err(AddressError::Ipv4NotSupported);
    }
    let scope = match scope_text {
        Some(text) => parse_scope(text)?,
        None => NO_SCOPE,
    };
    if is_link_local(&ip) && scope == NO_SCOPE {
        return Err(AddressError::MissingScope);
    }
    Ok(SocketAddrV6::new(ip, port, 0, scope))
}

/// The host part as `parse_address` reads it back, with the scope as `%index` when there is one;
/// the port is stored separately.
pub fn format_host(address: &SocketAddrV6) -> String {
    match address.scope_id() {
        NO_SCOPE => address.ip().to_string(),
        scope => format!("{}{SCOPE_SEPARATOR}{scope}", address.ip()),
    }
}

// `u32::from_str` accepts a leading `+`, which Windows would not.
fn parse_scope(text: &str) -> Result<u32, AddressError> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(AddressError::ScopeNotNumeric);
    }
    text.parse().map_err(|_| AddressError::ScopeNotNumeric)
}
