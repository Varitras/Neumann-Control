//! The worker that owns the connection to one speaker: one thread per speaker, commands from
//! the app before polling, and everything it learns sent back as events.

mod inbox;

use std::collections::HashSet;
use std::net::SocketAddrV6;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::catalog::{Catalog, Entry};
use crate::mask::{mask_host, mask_serial};
use crate::poll_schedule::{Due, Schedule};
use crate::schema_walk::{LeafReport, Progress, Walk, WalkError};
use crate::ssc::client::{Client, SscError};
use crate::ssc::wire::{DeviceError, SscPath};
use inbox::Inbox;

const DEFAULT_METER_INTERVAL: Duration = Duration::from_millis(100);
const DEFAULT_FAST_INTERVAL: Duration = Duration::from_secs(2);
const DEFAULT_SLOW_INTERVAL: Duration = Duration::from_secs(30);
/// About half a minute of KH 750 meter events.
const DEFAULT_UNREAD_EVENTS_LIMIT: usize = 1000;
const DEFAULT_RECONNECT_INTERVAL: Duration = Duration::from_secs(10);
/// How often a paused worker looks whether its events were read; whatever is due stays due.
const PAUSE_CHECK_INTERVAL: Duration = Duration::from_millis(50);

const PRODUCT_PATH: &str = "device/identity/product";
const VERSION_PATH: &str = "device/identity/version";
const SERIAL_PATH: &str = "device/identity/serial";
const NAME_PATH: &str = "device/name";

const NOT_CONNECTED_MESSAGE: &str = "the speaker is not connected";
const THREAD_NAME: &str = "speaker";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub address: SocketAddrV6,
    /// When set, a speaker reporting another serial is not used; unset, the serial reported on
    /// the first connection is expected from then on.
    pub expected_serial: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timing {
    pub meter: Duration,
    pub fast: Duration,
    pub slow: Duration,
    pub reconnect: Duration,
    /// Polling pauses while this many events wait unread, e.g. while the window is not drawn,
    /// so they cannot pile up; commands still run.
    pub unread_events_limit: usize,
}

impl Default for Timing {
    fn default() -> Timing {
        Timing {
            meter: DEFAULT_METER_INTERVAL,
            fast: DEFAULT_FAST_INTERVAL,
            slow: DEFAULT_SLOW_INTERVAL,
            reconnect: DEFAULT_RECONNECT_INTERVAL,
            unread_events_limit: DEFAULT_UNREAD_EVENTS_LIMIT,
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
    /// A read: the value as the speaker held it when the request was answered.
    Value {
        path: SscPath,
        value: Value,
    },
    /// The speaker's answer to one `Set`: the value it now holds. Each `Set` gets exactly one of
    /// `Written`, `WriteAbsent`, `WriteRejected` or `WriteFailed`, so the app can tell its own
    /// write confirmed from a read that was answered before the write arrived.
    Written {
        path: SscPath,
        value: Value,
    },
    /// A read found that the speaker does not have the path; it is not polled again on this
    /// connection.
    Absent(SscPath),
    /// The answer to one `Set` to a path the speaker does not have (also when a read found it
    /// missing before): nothing was written.
    WriteAbsent(SscPath),
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
    /// A running mean of the request round trips, sent at most once per second while connected.
    ResponseTime(Duration),
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
) -> (SpeakerHandle, Events) {
    let (command_sender, commands) = mpsc::channel();
    let (events, event_receiver) = mpsc::channel();
    let unread = Arc::new(AtomicUsize::new(0));
    let worker = Worker {
        host: mask_host(&target.address),
        target,
        timing,
        inbox: Inbox::new(commands),
        outbox: Outbox {
            events,
            unread: Arc::clone(&unread),
            wake,
        },
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
    let events = Events {
        receiver: event_receiver,
        unread,
    };
    (handle, events)
}

/// The worker's events, counting what is still unread so the worker can pause polling.
pub struct Events {
    receiver: Receiver<Event>,
    unread: Arc<AtomicUsize>,
}

impl Events {
    pub fn try_recv(&self) -> Result<Event, TryRecvError> {
        let event = self.receiver.try_recv()?;
        self.unread.fetch_sub(1, Ordering::Relaxed);
        Ok(event)
    }

    /// Every event that is already there, without waiting.
    pub fn try_iter(&self) -> impl Iterator<Item = Event> + '_ {
        std::iter::from_fn(|| self.try_recv().ok())
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Result<Event, RecvTimeoutError> {
        let event = self.receiver.recv_timeout(timeout)?;
        self.unread.fetch_sub(1, Ordering::Relaxed);
        Ok(event)
    }
}

struct Outbox {
    events: Sender<Event>,
    unread: Arc<AtomicUsize>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl Outbox {
    fn emit(&self, event: Event) {
        // Counted first, so the reader never takes it below zero.
        self.unread.fetch_add(1, Ordering::Relaxed);
        // A dropped receiver means the app is closing; the handle's Drop stops this thread.
        if self.events.send(event).is_err() {
            self.unread.fetch_sub(1, Ordering::Relaxed);
        }
        (self.wake)();
    }

    fn unread(&self) -> usize {
        self.unread.load(Ordering::Relaxed)
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
    /// A running "read everything", advanced whenever no command or poll is due.
    walk: Option<Walk>,
    response_time: ResponseTime,
}

/// Weight of the newest round trip in the running mean.
const RESPONSE_TIME_WEIGHT: f64 = 0.2;
const RESPONSE_TIME_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Default)]
struct ResponseTime {
    mean: Option<Duration>,
    reported_at: Option<Instant>,
}

impl ResponseTime {
    fn add(&mut self, sample: Duration) {
        let mean = match self.mean {
            Some(mean) => {
                mean.mul_f64(1.0 - RESPONSE_TIME_WEIGHT) + sample.mul_f64(RESPONSE_TIME_WEIGHT)
            }
            None => sample,
        };
        self.mean = Some(mean);
    }

    /// The mean when the last report is a second old or there was none.
    fn due(&mut self, now: Instant) -> Option<Duration> {
        let mean = self.mean?;
        let waited = self
            .reported_at
            .is_none_or(|at| now.duration_since(at) >= RESPONSE_TIME_INTERVAL);
        if !waited {
            return None;
        }
        self.reported_at = Some(now);
        Some(mean)
    }
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
        // Added by address alone: from now on this is the speaker, not whatever answers there.
        if self.target.expected_serial.is_none() && !identity.serial.is_empty() {
            self.target.expected_serial = Some(identity.serial.clone());
        }
        self.reported = None;
        let entries = Catalog::embedded().entries_for(&identity.product);
        self.outbox.emit(Event::Connected(identity));
        let mut session = Session {
            client,
            schedule: Schedule::new(&entries, &self.timing, Instant::now()),
            absent: HashSet::new(),
            walk: None,
            response_time: ResponseTime::default(),
        };
        let interruption = self.serve(&mut session, &entries);
        // The app waits for an answer to every "read everything".
        if session.walk.is_some() {
            self.outbox
                .emit(Event::AllValuesFailed(NOT_CONNECTED_MESSAGE.to_owned()));
        }
        match interruption {
            Interruption::Stopped => Err(Stopped),
            Interruption::Lost(error) => {
                self.report_unreachable(&error);
                Ok(())
            }
        }
    }

    fn serve(&self, session: &mut Session, entries: &[&Entry]) -> Interruption {
        // Commands go before polling here too: a strip takes clicks once its first values are
        // in, and the whole read takes about two seconds on a KH 750.
        for entry in entries {
            let step = self
                .run_waiting_commands(session)
                .and_then(|()| self.read(session, &entry.path));
            if let Err(interruption) = step {
                return interruption;
            }
        }
        loop {
            if let Err(interruption) = self.step(session) {
                return interruption;
            }
        }
    }

    /// Every command already sent, in order, without waiting for more.
    fn run_waiting_commands(&self, session: &mut Session) -> Result<(), Interruption> {
        loop {
            match self.inbox.next(Duration::ZERO) {
                Ok(command) => self.execute(session, command)?,
                Err(RecvTimeoutError::Timeout) => return Ok(()),
                Err(RecvTimeoutError::Disconnected) => return Err(Interruption::Stopped),
            }
        }
    }

    /// A pending command, or else what the schedule has due; waits for whichever comes first.
    fn step(&self, session: &mut Session) -> Result<(), Interruption> {
        let paused = self.paused();
        let walking = session.walk.is_some() && !paused;
        // Paused, what is due stays due and goes first once the events are read again.
        let wait = if paused {
            PAUSE_CHECK_INTERVAL
        } else if walking {
            Duration::ZERO
        } else {
            session.schedule.time_until_due(Instant::now())
        };
        match self.inbox.next(wait) {
            Ok(command) => return self.execute(session, command),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Err(Interruption::Stopped),
        }
        if self.paused() {
            return Ok(());
        }
        match session.schedule.take_due(Instant::now()) {
            Due::Meters(paths) => paths.iter().try_for_each(|path| self.read(session, path))?,
            Due::Path(path) => self.read(session, &path)?,
            Due::Nothing => {}
        }
        // One step per round, so neither the walk nor the meters can starve the other.
        let walked = self.advance_walk(session);
        self.report_response_time(session);
        walked
    }

    fn report_response_time(&self, session: &mut Session) {
        if let Some(sample) = session.client.take_round_trip() {
            session.response_time.add(sample);
        }
        if let Some(mean) = session.response_time.due(Instant::now()) {
            self.outbox.emit(Event::ResponseTime(mean));
        }
    }

    fn paused(&self) -> bool {
        self.outbox.unread() >= self.timing.unread_events_limit
    }

    fn execute(&self, session: &mut Session, command: Command) -> Result<(), Interruption> {
        match command {
            Command::Set { path, value } => self.write(session, path, value),
            Command::ReadAll => {
                session.walk.get_or_insert_with(Walk::new);
                Ok(())
            }
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
            Err(SscError::Device(error)) if error.code == DeviceError::NOT_FOUND => {
                if self.forget(session, path) {
                    self.outbox.emit(Event::Absent(path.clone()));
                }
            }
            Err(lost) if lost.is_connection_lost() => return Err(Interruption::Lost(lost)),
            // Nothing the user could act on; the next poll asks again.
            Err(error) => log::warn!("{} could not read {path}: {error}", self.host),
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
                self.outbox.emit(Event::Written {
                    path,
                    value: confirmed,
                });
                return Ok(());
            }
            Err(error) => error,
        };
        match error {
            // Reported even when already known: the app waits for an answer to its write.
            SscError::Device(error) if error.code == DeviceError::NOT_FOUND => {
                self.forget(session, &path);
                self.outbox.emit(Event::WriteAbsent(path));
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
            lost if lost.is_connection_lost() => {
                self.outbox.emit(Event::WriteFailed {
                    path,
                    message: lost.to_string(),
                });
                Err(Interruption::Lost(lost))
            }
            // Whether the device took the value is unknown; show what it holds now.
            unusable => {
                self.outbox.emit(Event::WriteFailed {
                    path: path.clone(),
                    message: unusable.to_string(),
                });
                self.read(session, &path)
            }
        }
    }

    fn advance_walk(&self, session: &mut Session) -> Result<(), Interruption> {
        let Some(walk) = session.walk.as_mut() else {
            return Ok(());
        };
        let should_stop = || self.inbox.stop_requested();
        let outcome = walk.advance(&mut session.client, &should_stop);
        if !matches!(outcome, Ok(Progress::Running)) {
            session.walk = None;
        }
        match outcome {
            Ok(Progress::Running) => Ok(()),
            Ok(Progress::Done(reports)) => {
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
