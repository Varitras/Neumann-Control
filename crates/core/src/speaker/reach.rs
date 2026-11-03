//! Reaching a link-local address through the other interfaces of this computer.

use std::net::SocketAddrV6;

/// The interfaces to try besides `named`, each once: one can hold several link-local addresses,
/// and an attempt can take the whole connect timeout.
pub(super) fn other_scopes(named: u32, locals: &[SocketAddrV6]) -> Vec<u32> {
    let mut scopes: Vec<u32> = locals
        .iter()
        .map(|local| local.scope_id())
        .filter(|scope| *scope != named)
        .collect();
    scopes.sort_unstable();
    scopes.dedup();
    scopes
}

/// The first address that answers. `stopped` is asked before each, as closing the app waits for
/// the worker and each attempt can take the whole connect timeout.
pub(super) fn first_answering<C, E>(
    candidates: impl IntoIterator<Item = SocketAddrV6>,
    stopped: impl Fn() -> bool,
    connect: impl Fn(SocketAddrV6) -> Result<C, E>,
) -> Option<(SocketAddrV6, C)> {
    for candidate in candidates {
        if stopped() {
            return None;
        }
        if let Ok(client) = connect(candidate) {
            return Some((candidate, client));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::net::Ipv6Addr;

    use super::*;

    fn address(scope: u32) -> SocketAddrV6 {
        SocketAddrV6::new(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1), 45, 0, scope)
    }

    /// An interface can hold several link-local addresses; trying it twice costs a timeout.
    #[test]
    fn each_other_interface_is_tried_once() {
        let locals = [address(7), address(18), address(18), address(53)];
        assert_eq!(other_scopes(7, &locals), [18, 53]);
    }

    /// Closing the app waits for the worker, and each attempt can take the whole connect timeout:
    /// a stop ends the row at the next address.
    #[test]
    fn a_stop_ends_the_row_of_addresses() {
        let tried = Cell::new(0);
        let found = first_answering(
            [address(18), address(53)],
            || tried.get() > 0,
            |_| {
                tried.set(tried.get() + 1);
                Err::<(), ()>(())
            },
        );
        assert_eq!(found, None);
        assert_eq!(tried.get(), 1);
    }

    #[test]
    fn the_first_address_that_answers_is_taken() {
        let answering = address(53);
        let found = first_answering(
            [address(18), answering],
            || false,
            |candidate| match candidate == answering {
                true => Ok("client"),
                false => Err(()),
            },
        );
        assert_eq!(found, Some((answering, "client")));
    }
}
