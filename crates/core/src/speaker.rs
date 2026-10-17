//! The worker that owns the connection to one speaker: one thread per speaker, commands from
//! the app before polling, and everything it learns sent back as events.

mod inbox;

use std::collections::HashSet;
use std::net::SocketAddrV6;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::catalog::{Catalog, Entry};
use crate::mask::{mask_host, mask_serial};
use crate::poll_schedule::{Due, Schedule};
use crate::schema_walk::{self, LeafReport, WalkError};
use crate::ssc::client::{Client, SscError};
use crate::ssc::wire::{DeviceError, SscPath};
use inbox::Inbox;

const DEFAULT_METER_INTERVAL: Duration = Duration::from_millis(100);
const DEFAULT_FAST_INTERVAL: Duration = Duration::from_secs(2);
const DEFAULT_SLOW_INTERVAL: Duration = Duration::from_secs(30);
const DEFAULT_RECONNECT_INTERVAL: Duration = Duration::from_secs(10);

const PRODUCT_PATH: &str = "device/identity/product";
const VERSION_PATH: &str = "device/identity/version";
const SERIAL_PATH: &str = "device/identity/serial";
const NAME_PATH: &str = "device/name";

const NOT_FOUND_CODE: i64 = 404;
const NOT_CONNECTED_MESSAGE: &str = "the speaker is not connected";
const THREAD_NAME: &str = "speaker";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub address: SocketAddrV6,
    /// When set, a speaker reporting another serial is not used.
    pub expected_serial: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timing {
    pub meter: Duration,
    pub fast: Duration,
    pub slow: Duration,
    pub reconnect: Duration,
}

impl Default for Timing {
    fn default() -> Timing {
        Timing {
            meter: DEFAULT_METER_INTERVAL,
            fast: DEFAULT_FAST_INTERVAL,
            slow: DEFAULT_SLOW_INTERVAL,
            reconnect: DEFAULT_RECONNECT_INTERVAL,
        }
    }
}

/// The value of a `Set` is checked against the catalog by the app before it is sent.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Set { path: SscPath, value: Value },
    ReadAll,
    Stop,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub product: String,
    pub version: String,
    pub serial: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Connected(Identity),
    /// Sent once per outage; the text is the masked error.
    Unreachable(String),
    Value {
        path: SscPath,
        value: Value,
    },
    /// The speaker does not have the path; it is not polled again on this connection, and
    /// every `Set` to it is answered with this again.
    Absent(SscPath),
    /// The speaker refused the write; a re-read follows.
    WriteRejected {
        path: SscPath,
        error: DeviceError,
    },
    /// The write did not reach the speaker or its answer got lost.
    WriteFailed {
        path: SscPath,
        message: String,
    },
    SerialMismatch {
        expected: String,
        found: String,
    },
    AllValues(Vec<LeafReport>),
    AllValuesFailed(String),
}

pub struct SpeakerHandle {
    commands: Sender<Command>,
    thread: Option<JoinHandle<()>>,
}

impl SpeakerHandle {
    pub fn send(&self, command: Command) {
        // After Stop or a panic the worker is gone and the command has no one to run it;
        // Drop logs a panic when joining.
        let _ = self.commands.send(command);
    }
}

impl Drop for SpeakerHandle {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Stop);
        let Some(thread) = self.thread.take() else {
            return;
        };
        if thread.join().is_err() {
            log::error!("speaker worker panicked");
        }
    }
}

/// Starts the worker thread. `wake` runs after every event, e.g. to repaint the UI.
pub fn spawn(
    target: Target,
    timing: Timing,
    wake: Arc<dyn Fn() + Send + Sync>,
) -> (SpeakerHandle, Receiver<Event>) {
    let (command_sender, commands) = mpsc::channel();
    let (events, event_receiver) = mpsc::channel();
    let worker = Worker {
        host: mask_host(&target.address),
        target,
        timing,
        inbox: Inbox::new(commands),
        outbox: Outbox { events, wake },
        reported: None,
    };
    let thread = thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || worker.run())
        .expect("the operating system starts a thread");
    let handle = SpeakerHandle {
        commands: command_sender,
        thread: Some(thread),
    };
    (handle, event_receiver)
}

struct Outbox {
    events: Sender<Event>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl Outbox {
    fn emit(&self, event: Event) {
        // A dropped receiver means the app is closing; the handle's Drop stops this thread.
        let _ = self.events.send(event);
        (self.wake)();
    }
}

/// Why the worker is disconnected, so each reason is reported once and not on every retry.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Disconnect {
    Unreachable,
    SerialMismatch,
}

/// Why a connected session ended.
enum Interruption {
    Stopped,
    /// The connection is gone, or the speaker cannot be used on it.
    Lost(SscError),
}

struct Stopped;

/// The state of one connection; a reconnect starts a new one.
struct Session {
    client: Client,
    schedule: Schedule,
    absent: HashSet<SscPath>,
}

struct Worker {
    target: Target,
    host: String,
    timing: Timing,
    inbox: Inbox,
    outbox: Outbox,
    reported: Option<Disconnect>,
}

impl Worker {
    fn run(mut self) {
        loop {
            if self.connect_and_serve().is_err() {
                return;
            }
            if self.wait_disconnected(self.timing.reconnect).is_err() {
                return;
            }
        }
    }

    /// One connection attempt and, if the speaker is the expected one, the session on it.
    fn connect_and_serve(&mut self) -> Result<(), Stopped> {
        let mut client = match Client::connect(self.target.address) {
            Ok(client) => client,
            Err(error) => {
                self.report_unreachable(&error);
                return Ok(());
            }
        };
        let identity = match self.read_identity(&mut client) {
            Ok(identity) => identity,
            Err(Interruption::Stopped) => return Err(Stopped),
            Err(Interruption::Lost(error)) => {
                self.report_unreachable(&error);
                return Ok(());
            }
        };
        if let Some(expected) = self.mismatched_serial(&identity) {
            self.report_mismatch(expected, identity.serial);
            return Ok(());
        }
        log::info!("connected to {}", self.host);
        self.reported = None;
        let entries = Catalog::embedded().entries_for(&identity.product);
        self.outbox.emit(Event::Connected(identity));
        let mut session = Session {
            client,
            schedule: Schedule::new(&entries, &self.timing, Instant::now()),
            absent: HashSet::new(),
        };
        match self.serve(&mut session, &entries) {
            Interruption::Stopped => Err(Stopped),
            Interruption::Lost(error) => {
                self.report_unreachable(&error);
                Ok(())
            }
        }
    }

    fn serve(&self, session: &mut Session, entries: &[&Entry]) -> Interruption {
        for entry in entries {
            if let Err(interruption) = self.read(session, &entry.path) {
                return interruption;
            }
        }
        loop {
            if let Err(interruption) = self.step(session) {
                return interruption;
            }
        }
    }

    /// A pending command, or else what the schedule has due; waits for whichever comes first.
    fn step(&self, session: &mut Session) -> Result<(), Interruption> {
        let wait = session.schedule.time_until_due(Instant::now());
        match self.inbox.next(wait) {
            Ok(command) => return self.execute(session, command),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Err(Interruption::Stopped),
        }
        match session.schedule.take_due(Instant::now()) {
            Due::Meters(paths) => paths.iter().try_for_each(|path| self.read(session, path)),
            Due::Path(path) => self.read(session, &path),
            Due::Nothing => Ok(()),
        }
    }

    fn execute(&self, session: &mut Session, command: Command) -> Result<(), Interruption> {
        match command {
            Command::Set { path, value } => self.write(session, path, value),
            Command::ReadAll => self.read_all(session),
            Command::Stop => Err(Interruption::Stopped),
        }
    }

    fn read(&self, session: &mut Session, path: &SscPath) -> Result<(), Interruption> {
        if self.inbox.stop_requested() {
            return Err(Interruption::Stopped);
        }
        match session.client.get(path) {
            Ok(value) => self.outbox.emit(Event::Value {
                path: path.clone(),
                value,
            }),
            Err(SscError::Device(error)) if error.code == NOT_FOUND_CODE => {
                if self.forget(session, path) {
                    self.outbox.emit(Event::Absent(path.clone()));
                }
            }
            // Nothing the user could act on; the next poll asks again.
            Err(SscError::Device(error)) => log::warn!(
                "{} refused to read {path} ({}): {}",
                self.host,
                error.code,
                error.description
            ),
            Err(lost) => return Err(Interruption::Lost(lost)),
        }
        Ok(())
    }

    fn write(
        &self,
        session: &mut Session,
        path: SscPath,
        value: Value,
    ) -> Result<(), Interruption> {
        let error = match session.client.set(&path, value) {
            Ok(confirmed) => {
                self.outbox.emit(Event::Value {
                    path,
                    value: confirmed,
                });
                return Ok(());
            }
            Err(error) => error,
        };
        match error {
            // Reported even when already known: the app waits for an answer to its write.
            SscError::Device(error) if error.code == NOT_FOUND_CODE => {
                self.forget(session, &path);
                self.outbox.emit(Event::Absent(path));
                Ok(())
            }
            // The device kept its old value (read-only, wrong type, ...); show that one again.
            SscError::Device(error) => {
                self.outbox.emit(Event::WriteRejected {
                    path: path.clone(),
                    error,
                });
                self.read(session, &path)
            }
            lost => {
                self.outbox.emit(Event::WriteFailed {
                    path,
                    message: lost.to_string(),
                });
                Err(Interruption::Lost(lost))
            }
        }
    }

    fn read_all(&self, session: &mut Session) -> Result<(), Interruption> {
        let should_stop = || self.inbox.stop_requested();
        match schema_walk::read_all(&mut session.client, &should_stop) {
            Ok(reports) => {
                self.outbox.emit(Event::AllValues(reports));
                Ok(())
            }
            Err(WalkError::Refused(reason)) => {
                self.outbox.emit(Event::AllValuesFailed(reason));
                Ok(())
            }
            Err(WalkError::Connection(error)) => {
                self.outbox.emit(Event::AllValuesFailed(error.to_string()));
                Err(Interruption::Lost(error))
            }
            Err(WalkError::Stopped) => Err(Interruption::Stopped),
        }
    }

    /// Stops polling `path`; false when it was already known to be absent.
    fn forget(&self, session: &mut Session, path: &SscPath) -> bool {
        if !session.absent.insert(path.clone()) {
            return false;
        }
        session.schedule.forget(path);
        true
    }

    /// Waits between connection attempts, answering commands at once since none can run.
    fn wait_disconnected(&self, duration: Duration) -> Result<(), Stopped> {
        let deadline = Instant::now() + duration;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.inbox.next(remaining) {
                Ok(Command::Set { path, .. }) => self.outbox.emit(Event::WriteFailed {
                    path,
                    message: NOT_CONNECTED_MESSAGE.to_owned(),
                }),
                Ok(Command::ReadAll) => self
                    .outbox
                    .emit(Event::AllValuesFailed(NOT_CONNECTED_MESSAGE.to_owned())),
                Ok(Command::Stop) | Err(RecvTimeoutError::Disconnected) => return Err(Stopped),
                Err(RecvTimeoutError::Timeout) => return Ok(()),
            }
        }
    }

    fn mismatched_serial(&self, identity: &Identity) -> Option<String> {
        self.target
            .expected_serial
            .clone()
            .filter(|expected| *expected != identity.serial)
    }

    fn report_unreachable(&mut self, error: &SscError) {
        if self.reported == Some(Disconnect::Unreachable) {
            return;
        }
        self.reported = Some(Disconnect::Unreachable);
        let reason = error.to_string();
        log::info!("{} is unreachable: {reason}", self.host);
        self.outbox.emit(Event::Unreachable(reason));
    }

    fn report_mismatch(&mut self, expected: String, found: String) {
        if self.reported == Some(Disconnect::SerialMismatch) {
            return;
        }
        self.reported = Some(Disconnect::SerialMismatch);
        log::info!(
            "{} has serial {}, expected {}",
            self.host,
            mask_serial(&found),
            mask_serial(&expected)
        );
        self.outbox.emit(Event::SerialMismatch { expected, found });
    }

    /// A device error counts as lost too: without its identity the speaker cannot be used.
    fn read_identity(&self, client: &mut Client) -> Result<Identity, Interruption> {
        Ok(Identity {
            product: self.read_text(client, PRODUCT_PATH)?,
            version: self.read_text(client, VERSION_PATH)?,
            serial: self.read_text(client, SERIAL_PATH)?,
            name: self.read_text(client, NAME_PATH)?,
        })
    }

    /// A string value as it is; any other value in its JSON form.
    fn read_text(&self, client: &mut Client, path: &str) -> Result<String, Interruption> {
        if self.inbox.stop_requested() {
            return Err(Interruption::Stopped);
        }
        let path = SscPath::parse(path).expect("constant identity path is valid");
        match client.get(&path).map_err(Interruption::Lost)? {
            Value::String(text) => Ok(text),
            other => Ok(other.to_string()),
        }
    }
}
