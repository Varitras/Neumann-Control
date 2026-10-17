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
    /// counts as `Stop`.
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
        true
    }

    pub(super) fn next(&self, timeout: Duration) -> Result<Command, RecvTimeoutError> {
        if self.stop_requested.get() {
            return Ok(Command::Stop);
        }
        if let Some(command) = self.pending.borrow_mut().pop_front() {
            return Ok(command);
        }
        self.receiver.recv_timeout(timeout)
    }
}
