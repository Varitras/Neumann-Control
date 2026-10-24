//! The worker's side of the command channel.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::time::Duration;

use super::Command;

/// The command channel plus the commands taken off it early while looking for a `Stop`, kept
/// in arrival order.
pub(super) struct Inbox {
    receiver: Receiver<Command>,
    pending: RefCell<VecDeque<Command>>,
    stop_requested: Cell<bool>,
}

impl Inbox {
    pub(super) fn new(receiver: Receiver<Command>) -> Inbox {
        Inbox {
            receiver,
            pending: RefCell::new(VecDeque::new()),
            stop_requested: Cell::new(false),
        }
    }

    /// Looks without waiting, so long work between polls can end early; a closed channel
    /// counts as `Stop`. Not while a command that came before the stop waits: the worker takes
    /// it through `next` first, so a write sent just before closing still goes out.
    pub(super) fn stop_requested(&self) -> bool {
        while !self.stop_requested.get() {
            match self.receiver.try_recv() {
                Ok(Command::Stop) | Err(TryRecvError::Disconnected) => {
                    self.stop_requested.set(true);
                }
                Ok(command) => self.pending.borrow_mut().push_back(command),
                Err(TryRecvError::Empty) => return false,
            }
        }
        self.pending.borrow().is_empty()
    }

    /// Commands that came before a `Stop` go first: a write sent just before a speaker is closed
    /// or removed still reaches it.
    pub(super) fn next(&self, timeout: Duration) -> Result<Command, RecvTimeoutError> {
        if let Some(command) = self.pending.borrow_mut().pop_front() {
            return Ok(command);
        }
        if self.stop_requested.get() {
            return Ok(Command::Stop);
        }
        self.receiver.recv_timeout(timeout)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::channel;

    use super::*;
    use crate::ssc::wire::SscPath;

    const WAIT: Duration = Duration::from_millis(10);

    /// A write sent before closing or removing a speaker reaches it before the worker stops.
    #[test]
    fn writes_that_came_before_stop_are_done_first() {
        let (sender, receiver) = channel();
        let set = Command::Set {
            path: SscPath::parse("audio/out/mute").unwrap(),
            value: serde_json::json!(true),
        };
        sender.send(set.clone()).unwrap();
        sender.send(Command::Stop).unwrap();
        let inbox = Inbox::new(receiver);
        // As the worker asks between two reads: not yet, the write goes first.
        assert!(!inbox.stop_requested());
        assert_eq!(inbox.next(WAIT), Ok(set));
        assert!(inbox.stop_requested());
        assert_eq!(inbox.next(WAIT), Ok(Command::Stop));
    }
}
