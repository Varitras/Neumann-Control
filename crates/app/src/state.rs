//! UI-independent state of one speaker: what the worker events add up to.
//! All times are seconds of the UI clock passed in by the caller.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::net::SocketAddrV6;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use neumann_control_core::discovery::Found;
use neumann_control_core::mask::{mask_host, mask_serial, mask_serial_in, mask_serial_like};
use neumann_control_core::schema_walk::LeafReport;
use neumann_control_core::settings::KnownSpeaker;
use neumann_control_core::speaker::{Command, Event, Identity};
use neumann_control_core::ssc::wire::SscPath;
use serde_json::Value;

use crate::display::capped;
use crate::gesture::GestureState;
use crate::history::{is_given, matching, merged};
use crate::meter::{MeterState, METER_FLOOR_DB};
use crate::paths;
use crate::status_leds::active_warnings;
use crate::trend::Trend;
use crate::writes::Refusal;

/// The KH 750 reports three output channels; a longer list from a faulty device would draw a
/// meter per element.
pub const MAX_METER_CHANNELS: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SpeakerKey {
    Serial(String),
    Address(SocketAddrV6),
}

/// "Komplett scannen": the walk, then its report file.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum FullScanState {
    #[default]
    Idle,
    Running,
    /// Read; the app writes the report and replaces this with the outcome.
    Read(Vec<LeafReport>),
    /// The JSON file's path.
    Written(String),
    Failed(ScanFailure),
}

/// Why "Komplett scannen" ended without a report; the raw reason (English, possibly with an
/// address in it) goes to the log only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanFailure {
    /// The speaker did not answer the walk through.
    Reading,
    /// The report could not be written next to the executable.
    Writing,
}

/// What the speaker's message is about, which decides when it ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MessageSource {
    /// Connecting, unreachable, another device: replaced by the next connection event.
    Connection,
    /// A refused, rejected or failed write. It stays until the user's next write request on
    /// this speaker (or a connection event): the worker re-reads a path right after a rejection
    /// and polls it every few seconds, so ending it on a fresh value hid it at once; and the
    /// texts do not name the path, so they are stale after any later request.
    Write,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Connecting,
    Connected,
    Unreachable,
}

/// The answer to "Alle Werte…": every value the speaker has, read on request.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum AllValuesState {
    #[default]
    Idle,
    Loading,
    Loaded(Vec<LeafReport>),
    Failed(String),
}

/// What the strip says about the speaker; worded only when drawn, so it follows the language.
#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    Unreachable,
    WriteFailed,
    /// The device refused a write with this error code.
    Rejected(i64),
    /// A different device answered at the address; its serial, masked.
    SerialMismatch(String),
    Refused(Refusal),
}

/// Write numbers for the whole session: the history keeps the changes of a speaker removed and
/// added again, and a refusal is matched by speaker and number.
static NEXT_WRITE: AtomicU64 = AtomicU64::new(0);

static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(0);

/// A write the speaker refused while later writes to its path were on their way, oldest first:
/// they took its value as their value before.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefusedPredecessor {
    pub write: u64,
    pub later: Vec<u64>,
}

#[derive(Clone, Debug)]
pub struct SpeakerState {
    pub key: SpeakerKey,
    /// Fixed while the speaker is listed, unlike the key, which changes once it reports its
    /// serial: its own windows are named by this, so they stay the same windows.
    pub instance: u64,
    pub address: SocketAddrV6,
    pub identity: Option<Identity>,
    /// What the last scan reported for this speaker; names may be empty.
    pub discovered: Option<Found>,
    /// What it reported when it last connected, from the settings; names it while it is off.
    pub remembered: Option<KnownSpeaker>,
    pub values: BTreeMap<SscPath, Value>,
    pub absent: BTreeSet<SscPath>,
    pub status: Status,
    /// When the current outage began (the UI clock); `None` while connected or connecting.
    pub unreachable_since: Option<f64>,
    pub message: Option<Message>,
    message_source: MessageSource,
    pub input_meters: Vec<MeterState>,
    pub output_meters: Vec<MeterState>,
    pub fader: GestureState,
    /// One per knob the user turned, so a sent value stays shown until the speaker reports it.
    pub knobs: BTreeMap<SscPath, GestureState>,
    pub all_values: AllValuesState,
    /// Case-insensitive part of a path the "Alle Werte" table is narrowed to.
    pub all_values_filter: String,
    /// The worker's running mean; none until the current connection measured one.
    pub response_time: Option<Duration>,
    /// What was sent since the history last took it, at the end of `logic` and of `ui`.
    pub written: Vec<Written>,
    /// Sent values the speaker has not answered yet; the value before of the next write.
    pending: BTreeMap<SscPath, Pending>,
    /// Writes the old workers of a moved speaker still answer: by worker, then by path in the
    /// order sent.
    retired: BTreeMap<u64, BTreeMap<SscPath, VecDeque<u64>>>,
    next_retired: u64,
    /// The number of the write sent last.
    newest_write: Option<u64>,
    /// The numbers of writes the speaker refused, that failed, or that will never be answered
    /// (the connection went away), since the app last took them.
    pub refused_writes: Vec<u64>,
    /// Refused writes later writes took their value before from, since the app last took them.
    pub refused_predecessors: Vec<RefusedPredecessor>,
    /// `device/temperature` as reported (kelvin), for the device panel's trend.
    pub temperature: Trend<f64>,
    /// The active warnings, empty while there are none.
    pub warnings: Trend<Vec<String>>,
    pub full_scan: FullScanState,
}

/// One write sent this frame, as the history files it.
#[derive(Clone, Debug, PartialEq)]
pub struct Written {
    pub path: SscPath,
    pub before: Option<Value>,
    pub after: Value,
    /// Its number, to follow the speaker's answer.
    pub write: u64,
}

/// The newest value sent to one path and the numbers of its `Set`s still unanswered, oldest
/// first: the worker sends them in order and the speaker answers each.
#[derive(Clone, Debug, PartialEq)]
struct Pending {
    value: Value,
    unanswered: VecDeque<u64>,
    /// What each unanswered `Set` sent, in the same order: a refused one is taken out of
    /// `value` again.
    sent: VecDeque<Value>,
    /// What the answers so far leave on the speaker: its report before the first `Set`, with
    /// each confirmed one merged in, one with an answer that tells nothing taken as applied,
    /// and a refused or not found one left out. Its report is not taken while writes are on
    /// their way, so `value` is built again from this after a refusal.
    base: Option<Value>,
}

/// How an answered `Set` counts for the value still on its way to its path.
enum Answer<'a> {
    /// The speaker holds this now.
    Held(&'a Value),
    /// Failed with an answer that tells nothing while the connection stays: it may have been
    /// applied. A lost connection forgets everything on its way anyway.
    MaybeApplied,
    NotApplied,
}

impl SpeakerState {
    pub fn new(key: SpeakerKey, address: SocketAddrV6) -> SpeakerState {
        SpeakerState {
            key,
            instance: NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed),
            address,
            identity: None,
            discovered: None,
            remembered: None,
            values: BTreeMap::new(),
            absent: BTreeSet::new(),
            status: Status::Connecting,
            unreachable_since: None,
            message: None,
            message_source: MessageSource::Connection,
            input_meters: Vec::new(),
            output_meters: Vec::new(),
            fader: GestureState::default(),
            knobs: BTreeMap::new(),
            all_values: AllValuesState::Idle,
            all_values_filter: String::new(),
            response_time: None,
            written: Vec::new(),
            pending: BTreeMap::new(),
            retired: BTreeMap::new(),
            next_retired: 0,
            newest_write: None,
            refused_writes: Vec::new(),
            refused_predecessors: Vec::new(),
            temperature: Trend::default(),
            warnings: Trend::default(),
            full_scan: FullScanState::Idle,
        }
    }

    /// The speaker's own name, else the announced one, else the one it had when it last
    /// connected, else the model, else the masked address; any of these names can be empty.
    pub fn display_name(&self) -> String {
        let own = self
            .identity
            .as_ref()
            .map(|identity| identity.name.as_str());
        let announced = self.discovered.as_ref().map(|found| found.name.as_str());
        let remembered = self.remembered.as_ref().map(|known| known.name.as_str());
        [own, announced, remembered, self.model()]
            .into_iter()
            .flatten()
            .find(|name| !name.is_empty())
            .map(|name| capped(&self.with_serial_masked(name)))
            .unwrap_or_else(|| mask_host(&self.address))
    }

    /// A name may hold the serial (factory names do); it is shown masked like the announced
    /// name. Without a known serial, digits as long as a serial are masked.
    fn with_serial_masked(&self, name: &str) -> String {
        let serial = self
            .identity
            .as_ref()
            .map(|identity| identity.serial.as_str())
            .or(match &self.key {
                SpeakerKey::Serial(serial) => Some(serial.as_str()),
                SpeakerKey::Address(_) => None,
            })
            .filter(|serial| !serial.is_empty());
        match serial {
            Some(serial) => mask_serial_in(name, serial),
            None => mask_serial_like(name),
        }
    }

    /// The product the speaker itself reported. Writes and the catalog rows use only this,
    /// like the worker's polling: the announced model is unchecked mDNS text.
    pub fn product(&self) -> &str {
        self.identity
            .as_ref()
            .map_or("", |identity| identity.product.as_str())
    }

    /// The reported product, else the announced one, else the one kept from an earlier
    /// connection; for showing only.
    pub fn model(&self) -> Option<&str> {
        let reported = self
            .identity
            .as_ref()
            .map(|identity| identity.product.as_str());
        let announced = self
            .discovered
            .as_ref()
            .and_then(|found| found.product.as_deref());
        let remembered = self.remembered.as_ref().map(|known| known.product.as_str());
        [reported, announced, remembered]
            .into_iter()
            .flatten()
            .find(|model| !model.is_empty())
    }

    /// Notes a sent write for the history; Identify is a momentary signal, not a setting.
    pub(crate) fn record_written(&mut self, path: &SscPath, value: &Value) {
        if *path == paths::path(paths::IDENTIFY) {
            return;
        }
        let base = self.latest(path);
        let before = base.map(|base| matching(base, value)).filter(is_given);
        let merged = merged(base, value);
        let reported = self.values.get(path).cloned();
        let (mut unanswered, mut sent, base) = self
            .pending
            .remove(path)
            .map(|pending| (pending.unanswered, pending.sent, pending.base))
            .unwrap_or((VecDeque::new(), VecDeque::new(), reported));
        let write = NEXT_WRITE.fetch_add(1, Ordering::Relaxed);
        unanswered.push_back(write);
        sent.push_back(value.clone());
        self.newest_write = Some(write);
        self.written.push(Written {
            path: path.clone(),
            before,
            after: value.clone(),
            write,
        });
        let pending = Pending {
            value: merged,
            unanswered,
            sent,
            base,
        };
        self.pending.insert(path.clone(), pending);
    }

    /// The number of the write sent last, to follow its answer.
    pub fn newest_write(&self) -> Option<u64> {
        self.newest_write
    }

    /// The speaker moved: a new worker takes over, the old one answers what it already took
    /// (`apply_retired`) until its connection ends (`end_retired`). Its writes wait apart from
    /// the new worker's and from an earlier old one's, each answered in its own order; the
    /// number returned names this old worker.
    pub fn retire_outstanding(&mut self) -> u64 {
        let pending = std::mem::take(&mut self.pending);
        let writes = pending
            .into_iter()
            .map(|(path, pending)| (path, pending.unanswered))
            .collect();
        let worker = self.next_retired;
        self.next_retired += 1;
        self.retired.insert(worker, writes);
        self.forget_outstanding();
        worker
    }

    /// An event from the old worker `worker`: only its answers to writes count. A confirmed
    /// write, and one that failed with the old connection, may have reached the speaker after
    /// the new worker read the path, so the new worker reads it again (the returned command);
    /// the old answer is not taken over, since a newer write may have been answered meanwhile.
    pub fn apply_retired(&mut self, worker: u64, event: &Event) -> Option<Command> {
        let (path, done, maybe_applied) = match event {
            Event::Written { path, .. } => (path, true, true),
            Event::WriteFailed { path, .. } => (path, false, true),
            Event::WriteAbsent(path) | Event::WriteRejected { path, .. } => (path, false, false),
            _ => return None,
        };
        let writes = self
            .retired
            .get_mut(&worker)
            .and_then(|writes| writes.get_mut(path))?;
        let write = writes.pop_front()?;
        let later = writes.iter().copied().collect();
        if matches!(event, Event::WriteRejected { .. }) {
            self.note_predecessor(Some(write), later);
        }
        if !done {
            self.refused_writes.push(write);
        }
        maybe_applied.then(|| Command::Read(path.clone()))
    }

    /// The old worker `worker` is gone: what it did not answer was not written.
    pub fn end_retired(&mut self, worker: u64) {
        let Some(writes) = self.retired.remove(&worker) else {
            return;
        };
        self.refused_writes.extend(writes.into_values().flatten());
    }

    /// Whether the write numbered `write` is still unanswered. A write whose answer will not
    /// come (the connection was lost) is not.
    pub fn awaits_write(&self, write: u64) -> bool {
        let pending = self
            .pending
            .values()
            .any(|pending| pending.unanswered.contains(&write));
        let mut retired = self.retired.values().flat_map(BTreeMap::values);
        pending || retired.any(|writes| writes.contains(&write))
    }

    /// The newest sent value not yet answered, else the reported one: what the speaker holds
    /// once its writes are through. The next toggle and a take-back start from this, so a write
    /// still on the way does not count as someone else's change.
    pub fn latest(&self, path: &SscPath) -> Option<&Value> {
        let sent = self.pending.get(path).map(|pending| &pending.value);
        sent.or_else(|| self.values.get(path))
    }

    /// Every path as `latest` gives it, for planning writes against what the speaker will hold.
    pub fn latest_values(&self) -> BTreeMap<SscPath, Value> {
        let mut values = self.values.clone();
        for (path, pending) in &self.pending {
            values.insert(path.clone(), pending.value.clone());
        }
        values
    }

    /// Counts the oldest `Set` to `path` as answered and returns its number; when none is left,
    /// the sent value is no longer held. Whether the speaker took it, its next report says.
    fn settle(&mut self, path: &SscPath, answer: Answer) -> Option<u64> {
        let mut answered = None;
        if let Some(pending) = self.pending.get_mut(path) {
            answered = pending.unanswered.pop_front();
            let sent = pending.sent.pop_front();
            let taken = match answer {
                Answer::Held(value) => Some(value.clone()),
                Answer::MaybeApplied => sent,
                Answer::NotApplied => None,
            };
            if let Some(taken) = taken {
                pending.base = Some(merged(pending.base.as_ref(), &taken));
            }
            if !pending.unanswered.is_empty() {
                return answered;
            }
            self.pending.remove(path);
        }
        if *path == paths::path(paths::OUTPUT_LEVEL) {
            self.fader.confirmed();
        }
        if let Some(knob) = self.knobs.get_mut(path) {
            knob.confirmed();
        }
        answered
    }

    /// The value still on its way to `path`, built again from what the answers so far leave
    /// and the writes not yet answered, without the one just refused.
    fn drop_refused_from_pending(&mut self, path: &SscPath) {
        let Some(pending) = self.pending.get_mut(path) else {
            return;
        };
        let value = pending
            .sent
            .iter()
            .fold(pending.base.clone(), |base, sent| {
                Some(merged(base.as_ref(), sent))
            });
        if let Some(value) = value {
            pending.value = value;
        }
    }

    /// Only for an explicit refusal: a write that failed with the connection may have been
    /// applied.
    fn note_predecessor(&mut self, refused: Option<u64>, later: Vec<u64>) {
        let Some(write) = refused else {
            return;
        };
        if later.is_empty() {
            return;
        }
        let predecessor = RefusedPredecessor { write, later };
        self.refused_predecessors.push(predecessor);
    }

    /// No answer is coming for what is pending: those writes count as failed.
    fn drop_pending(&mut self) {
        let pending = std::mem::take(&mut self.pending);
        let numbers = pending.into_values().flat_map(|pending| pending.unanswered);
        self.refused_writes.extend(numbers);
    }

    fn take_reported(&mut self, path: SscPath, value: Value, now: f64) {
        self.absent.remove(&path);
        self.feed_meters(&path, &value, now);
        self.record_trends(&path, &value, now);
        self.values.insert(path, value);
    }

    /// Forgets what the worker was asked and will no longer answer (the connection is gone or
    /// the worker was replaced for a moved speaker): sent writes no longer hold their paths, and
    /// a running read or scan can be asked again.
    pub fn forget_outstanding(&mut self) {
        self.drop_pending();
        if self.all_values == AllValuesState::Loading {
            self.all_values = AllValuesState::Idle;
        }
        if self.full_scan == FullScanState::Running {
            self.full_scan = FullScanState::Idle;
        }
    }

    pub fn apply(&mut self, event: Event, now: f64) {
        match event {
            Event::Connected(identity) => {
                if !identity.serial.is_empty() {
                    self.key = SpeakerKey::Serial(identity.serial.clone());
                }
                self.identity = Some(identity);
                self.status = Status::Connected;
                self.unreachable_since = None;
                self.set_message(None, MessageSource::Connection);
            }
            Event::Reached(address) => self.address = address,
            // The masked reason is for the log; the strip only shows the generic text.
            Event::Unreachable(_) => {
                self.status = Status::Unreachable;
                self.unreachable_since.get_or_insert(now);
                self.set_message(Some(Message::Unreachable), MessageSource::Connection);
                self.reset_meters();
                self.response_time = None;
                self.forget_outstanding();
                self.temperature.record(now, None);
                self.warnings.record(now, None);
            }
            // The worker answers one request at a time, so a read answered before a write was
            // sent can arrive after it; the write's own answer follows and wins.
            Event::Value { path, value } => {
                if !self.pending.contains_key(&path) {
                    self.take_reported(path, value, now);
                }
            }
            Event::Written { path, value } => {
                self.settle(&path, Answer::Held(&value));
                if !self.pending.contains_key(&path) {
                    self.take_reported(path, value, now);
                }
            }
            // A write on its way gets its own answer, which may still be `Written`.
            Event::Absent(path) => {
                self.values.remove(&path);
                self.absent.insert(path);
            }
            Event::WriteAbsent(path) => {
                let refused = self.settle(&path, Answer::NotApplied);
                self.refused_writes.extend(refused);
                self.values.remove(&path);
                self.absent.insert(path);
            }
            // A re-read or, for a lost connection, the reconnect follows; it shows the real value.
            Event::WriteRejected { path, error } => {
                let refused = self.settle(&path, Answer::NotApplied);
                let later = self
                    .pending
                    .get(&path)
                    .map(|pending| pending.unanswered.iter().copied().collect());
                self.note_predecessor(refused, later.unwrap_or_default());
                self.refused_writes.extend(refused);
                self.drop_refused_from_pending(&path);
                let message = Message::Rejected(error.code);
                self.set_message(Some(message), MessageSource::Write);
            }
            Event::WriteFailed { path, .. } => {
                let refused = self.settle(&path, Answer::MaybeApplied);
                self.refused_writes.extend(refused);
                // The connection's own message (unreachable, another serial) says more.
                let told =
                    self.message_source == MessageSource::Connection && self.message.is_some();
                if !told {
                    self.set_message(Some(Message::WriteFailed), MessageSource::Write);
                }
            }
            Event::SerialMismatch { found, .. } => {
                self.status = Status::Unreachable;
                self.unreachable_since.get_or_insert(now);
                self.response_time = None;
                self.drop_pending();
                let message = Message::SerialMismatch(mask_serial(&found));
                self.set_message(Some(message), MessageSource::Connection);
                self.temperature.record(now, None);
                self.warnings.record(now, None);
            }
            Event::AllValues(reports) => {
                if self.full_scan == FullScanState::Running {
                    self.full_scan = FullScanState::Read(reports.clone());
                }
                self.finish_all_values(AllValuesState::Loaded(reports));
            }
            Event::AllValuesFailed(reason) => {
                if self.full_scan == FullScanState::Running {
                    log::warn!("full scan ended early: {reason}");
                    self.full_scan = FullScanState::Failed(ScanFailure::Reading);
                }
                self.finish_all_values(AllValuesState::Failed(reason));
            }
            Event::ResponseTime(time) => self.response_time = Some(time),
        }
    }

    fn record_trends(&mut self, path: &SscPath, value: &Value, now: f64) {
        if *path == paths::path(paths::TEMPERATURE) {
            self.temperature.record(now, value.as_f64());
        } else if *path == paths::path(paths::WARNINGS) {
            self.warnings.record(now, active_warnings(value));
        }
    }

    /// "Alle Werte" may be (re)read: only from a connected speaker, once at a time.
    pub fn can_read_all_values(&self) -> bool {
        self.status == Status::Connected && self.all_values != AllValuesState::Loading
    }

    /// Starts reading every value; the worker answers with `AllValues` or `AllValuesFailed`.
    pub fn read_all_values(&mut self) -> Command {
        self.all_values = AllValuesState::Loading;
        Command::ReadAll
    }

    /// Starts "Komplett scannen"; the walk is the same as for "Alle Werte".
    pub fn start_full_scan(&mut self) -> Command {
        self.full_scan = FullScanState::Running;
        Command::ReadAll
    }

    /// Closes the "Alle Werte" window and forgets its filter.
    pub fn close_all_values(&mut self) {
        self.all_values = AllValuesState::Idle;
        self.all_values_filter.clear();
    }

    /// An answer that comes after the window was closed would reopen it.
    fn finish_all_values(&mut self, outcome: AllValuesState) {
        if self.all_values == AllValuesState::Loading {
            self.all_values = outcome;
        }
    }

    /// The loudest input channel as its meter shows it now; none before any meter reading.
    pub fn loudest_input(&self, now: f64) -> Option<f64> {
        self.input_meters
            .iter()
            .map(|meter| meter.shown(now))
            .reduce(f64::max)
    }

    pub(crate) fn set_message(&mut self, message: Option<Message>, source: MessageSource) {
        self.message = message;
        self.message_source = source;
    }

    /// A refused write's message. "Not connected" says less than the connection's own message
    /// (unreachable, another serial), which stays.
    pub(crate) fn tell_refusal(&mut self, refusal: Refusal) {
        let told = refusal == Refusal::NotConnected
            && self.message_source == MessageSource::Connection
            && self.message.is_some();
        if !told {
            self.set_message(Some(Message::Refused(refusal)), MessageSource::Write);
        }
    }

    /// A new write request makes the message about the previous one stale.
    pub(crate) fn clear_write_message(&mut self) {
        if self.message_source == MessageSource::Write {
            self.set_message(None, MessageSource::Write);
        }
    }

    fn reset_meters(&mut self) {
        let meters = self.input_meters.iter_mut().chain(&mut self.output_meters);
        for meter in meters {
            meter.reset();
        }
    }

    fn feed_meters(&mut self, path: &SscPath, value: &Value, now: f64) {
        let meters = match path.to_string().as_str() {
            paths::INPUT_METER => &mut self.input_meters,
            paths::OUTPUT_METER => &mut self.output_meters,
            _ => return,
        };
        let Some(levels) = value.as_array() else {
            return;
        };
        let levels = &levels[..levels.len().min(MAX_METER_CHANNELS)];
        meters.resize_with(levels.len(), MeterState::default);
        for (meter, level) in meters.iter_mut().zip(levels) {
            meter.update(level.as_f64().unwrap_or(METER_FLOOR_DB), now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::VALUE_CHARACTER_LIMIT;
    use neumann_control_core::address::parse_address;
    use neumann_control_core::ssc::wire::DeviceError;
    use serde_json::json;

    const SERIAL: &str = "SIM0001234";
    const OTHER_SERIAL: &str = "SIM0007500";
    const ADDRESS_TEXT: &str = "fe80::2a36:38ff:fe12:3456%7";
    const PORT: u16 = neumann_control_core::address::SSC_PORT;

    fn address(text: &str) -> SocketAddrV6 {
        parse_address(text, PORT).unwrap()
    }

    fn found(text: &str, serial: Option<&str>) -> Found {
        Found {
            name: "KH 120".to_owned(),
            product: None,
            serial: serial.map(str::to_owned),
            address: address(text),
        }
    }

    fn state() -> SpeakerState {
        SpeakerState::new(
            SpeakerKey::Address(address(ADDRESS_TEXT)),
            address(ADDRESS_TEXT),
        )
    }

    fn value_event(text: &str, value: Value) -> Event {
        Event::Value {
            path: path(text),
            value,
        }
    }

    fn path(text: &str) -> SscPath {
        SscPath::parse(text).unwrap()
    }

    #[test]
    fn temperature_and_warnings_are_recorded_with_gaps() {
        let mut speaker = state();
        speaker.apply(value_event("device/temperature", json!(310)), 1.0);
        speaker.apply(value_event("warnings", json!(["NO_WARNING"])), 1.0);
        speaker.apply(value_event("warnings", json!(["OVERTEMPERATURE"])), 2.0);
        speaker.apply(Event::Unreachable("gone".to_owned()), 3.0);
        let temperature: Vec<_> = speaker
            .temperature
            .points()
            .map(|point| (point.time, point.value))
            .collect();
        assert_eq!(temperature, vec![(1.0, Some(310.0)), (3.0, None)]);
        let warnings: Vec<_> = speaker
            .warnings
            .points()
            .map(|point| (point.time, point.value.clone()))
            .collect();
        let over = vec!["OVERTEMPERATURE".to_owned()];
        assert_eq!(
            warnings,
            vec![(1.0, Some(Vec::new())), (2.0, Some(over)), (3.0, None)]
        );
    }

    #[test]
    fn connected_upgrades_key_to_serial_and_keeps_address() {
        let mut speaker = state();
        speaker.apply(
            Event::Connected(Identity {
                product: "KH 120 II".to_owned(),
                version: "1".to_owned(),
                serial: SERIAL.to_owned(),
                name: "left".to_owned(),
            }),
            0.0,
        );
        assert_eq!(speaker.key, SpeakerKey::Serial(SERIAL.to_owned()));
        assert_eq!(speaker.address, address(ADDRESS_TEXT));
        assert_eq!(speaker.status, Status::Connected);
    }

    #[test]
    fn a_speaker_reached_through_another_interface_keeps_that_address() {
        let mut speaker = state();
        let reached = address("fe80::2a36:38ff:fe12:3456%18");
        speaker.apply(Event::Reached(reached), 0.0);
        assert_eq!(speaker.address, reached);
    }

    #[test]
    fn unreachable_event_greys_the_speaker_and_keeps_values() {
        let mut speaker = state();
        speaker.apply(
            Event::Value {
                path: path("device/name"),
                value: json!("left"),
            },
            0.0,
        );
        speaker.apply(Event::Unreachable("masked".to_owned()), 1.0);
        assert_eq!(speaker.status, Status::Unreachable);
        assert_eq!(speaker.message, Some(Message::Unreachable));
        assert_eq!(speaker.values.len(), 1);
    }

    #[test]
    fn absent_event_removes_value() {
        let mut speaker = state();
        speaker.apply(
            Event::Value {
                path: path("device/name"),
                value: json!("left"),
            },
            0.0,
        );
        speaker.apply(Event::Absent(path("device/name")), 1.0);
        assert!(speaker.values.is_empty());
        assert!(speaker.absent.contains(&path("device/name")));
    }

    #[test]
    fn a_rejected_write_sets_the_rejection_message() {
        let mut speaker = state();
        speaker.apply(
            Event::WriteRejected {
                path: path("device/name"),
                error: DeviceError {
                    code: DeviceError::READ_ONLY,
                    description: String::new(),
                },
            },
            0.0,
        );
        assert_eq!(
            speaker.message,
            Some(Message::Rejected(DeviceError::READ_ONLY))
        );
    }

    #[test]
    fn serial_mismatch_masks_found_serial() {
        let mut speaker = state();
        speaker.apply(
            Event::SerialMismatch {
                expected: SERIAL.to_owned(),
                found: OTHER_SERIAL.to_owned(),
            },
            0.0,
        );
        assert_eq!(speaker.status, Status::Unreachable);
        let Some(Message::SerialMismatch(masked)) = speaker.message else {
            panic!("{:?}", speaker.message);
        };
        assert_eq!(masked, mask_serial(OTHER_SERIAL));
        assert_ne!(masked, OTHER_SERIAL);
    }

    #[test]
    fn response_time_is_cleared_when_unreachable() {
        let mut speaker = state();
        speaker.apply(Event::ResponseTime(Duration::from_millis(14)), 0.0);
        assert_eq!(speaker.response_time, Some(Duration::from_millis(14)));
        speaker.apply(Event::Unreachable("gone".to_owned()), 1.0);
        assert_eq!(speaker.response_time, None);
    }

    #[test]
    fn a_long_meter_list_draws_a_bounded_number_of_meters() {
        let mut speaker = state();
        let levels: Vec<f64> = vec![-20.0; 500];
        speaker.apply(
            Event::Value {
                path: path("m/in/level"),
                value: json!(levels),
            },
            0.0,
        );
        assert_eq!(speaker.input_meters.len(), MAX_METER_CHANNELS);
    }

    #[test]
    fn a_huge_device_name_is_shortened_for_the_strip() {
        let mut speaker = state();
        let huge = "n".repeat(1_000_000);
        speaker.apply(Event::Connected(identity(&huge, "KH 120 II")), 0.0);
        assert!(speaker.display_name().chars().count() <= VALUE_CHARACTER_LIMIT + 1);
    }

    #[test]
    fn meter_value_feeds_one_meter_per_channel() {
        let mut speaker = state();
        speaker.apply(
            Event::Value {
                path: path("m/out/level"),
                value: json!([-12.0, -20.5]),
            },
            0.0,
        );
        assert_eq!(speaker.output_meters.len(), 2);
        assert_eq!(speaker.output_meters[1].level(), -20.5);
    }

    /// A band write still on its way counts merged into the reported array, as the speaker
    /// will hold it; paths without a write stay as reported.
    #[test]
    fn the_latest_values_hold_writes_on_their_way_merged() {
        let mut speaker = state();
        let gain = path("audio/out/eq2/gain");
        let mute = path(paths::MUTE);
        for (path, value) in [(&gain, json!([0.0, 3.0])), (&mute, json!(false))] {
            speaker.apply(
                Event::Value {
                    path: path.clone(),
                    value,
                },
                0.0,
            );
        }
        speaker.record_written(&gain, &json!([5.0, null]));
        let latest = speaker.latest_values();
        assert_eq!(latest.get(&gain), Some(&json!([5.0, 3.0])));
        assert_eq!(latest.get(&mute), Some(&json!(false)));
    }

    /// Every answer the worker can give to a `Set`: each settles the one write it answers, and
    /// only a confirmation leaves it done.
    #[test]
    fn every_answer_but_a_confirmation_refuses_its_write() {
        let mute = path(paths::MUTE);
        let answers = [
            ("absent", Event::WriteAbsent(mute.clone()), true),
            (
                "rejected",
                Event::WriteRejected {
                    path: mute.clone(),
                    error: DeviceError {
                        code: DeviceError::READ_ONLY,
                        description: String::new(),
                    },
                },
                true,
            ),
            (
                "failed",
                Event::WriteFailed {
                    path: mute.clone(),
                    message: "timeout".to_owned(),
                },
                true,
            ),
            (
                "written",
                Event::Written {
                    path: mute.clone(),
                    value: json!(true),
                },
                false,
            ),
        ];
        for (name, answer, refuses) in answers {
            let mut speaker = state();
            speaker.apply(Event::Connected(identity("left", "KH 120 II")), 0.0);
            speaker.record_written(&mute, &json!(true));
            let write = speaker.newest_write().unwrap();
            speaker.apply(answer, 1.0);
            assert_eq!(
                speaker.refused_writes.contains(&write),
                refuses,
                "{name}: {:?}",
                speaker.refused_writes
            );
            assert!(!speaker.awaits_write(write), "{name}");
        }
    }

    /// Two writes on a path the speaker lacks: each 404 refuses the one it answers.
    #[test]
    fn a_404_to_a_write_refuses_only_that_write() {
        let mut speaker = state();
        speaker.apply(Event::Connected(identity("left", "KH 120 II")), 0.0);
        let mute = path(paths::MUTE);
        speaker.record_written(&mute, &json!(true));
        let first = speaker.newest_write().unwrap();
        speaker.record_written(&mute, &json!(false));
        let second = speaker.newest_write().unwrap();
        speaker.apply(Event::WriteAbsent(mute.clone()), 1.0);
        assert_eq!(speaker.refused_writes, vec![first]);
        assert!(speaker.awaits_write(second));
        speaker.apply(Event::WriteAbsent(mute), 1.1);
        assert_eq!(speaker.refused_writes, vec![first, second]);
    }

    /// A read found the path missing just before a write went out, and the write then went
    /// through: the read's 404 is no answer to the write, which stays written.
    #[test]
    fn a_404_to_a_read_leaves_a_write_on_its_way_alone() {
        let mut speaker = state();
        speaker.apply(Event::Connected(identity("left", "KH 120 II")), 0.0);
        let mute = path(paths::MUTE);
        speaker.record_written(&mute, &json!(true));
        let write = speaker.newest_write().unwrap();
        speaker.apply(Event::Absent(mute.clone()), 1.0);
        assert!(speaker.awaits_write(write));
        speaker.apply(
            Event::Written {
                path: mute.clone(),
                value: json!(true),
            },
            1.1,
        );
        assert!(speaker.refused_writes.is_empty());
        assert_eq!(speaker.values.get(&mute), Some(&json!(true)));
    }

    /// A write failed because the connection went: "unreachable" says more and stays, as for
    /// a refused write (`tell_refusal`).
    #[test]
    fn a_write_failed_in_an_outage_leaves_the_outage_message() {
        let mut speaker = state();
        speaker.apply(Event::Connected(identity("left", "KH 120 II")), 0.0);
        let mute = path(paths::MUTE);
        speaker.record_written(&mute, &json!(true));
        speaker.apply(Event::Unreachable("gone".to_owned()), 1.0);
        speaker.apply(
            Event::WriteFailed {
                path: mute,
                message: "not connected".to_owned(),
            },
            1.1,
        );
        assert_eq!(speaker.message, Some(Message::Unreachable));
    }

    /// The connection went away with a write on its way: the worker fails it after reporting
    /// the outage, and that failure still counts for the write.
    #[test]
    fn a_write_outstanding_at_an_outage_counts_as_failed() {
        let mut speaker = state();
        speaker.apply(Event::Connected(identity("left", "KH 120 II")), 0.0);
        let mute = path(paths::MUTE);
        speaker.record_written(&mute, &json!(true));
        let write = speaker.newest_write().unwrap();
        speaker.apply(Event::Unreachable("gone".to_owned()), 1.0);
        speaker.apply(
            Event::WriteFailed {
                path: mute,
                message: "not connected".to_owned(),
            },
            1.1,
        );
        assert!(speaker.refused_writes.contains(&write));
    }

    /// A speaker removed and added again is a new state, but its history stays: its writes are
    /// numbered on from the old ones, so a refusal of a new write closes no old change.
    #[test]
    fn a_new_state_numbers_its_writes_on_from_the_old_ones() {
        let mute = path(paths::MUTE);
        let mut before = state();
        before.record_written(&mute, &json!(true));
        let old = before.newest_write().unwrap();
        let mut after = state();
        after.record_written(&mute, &json!(true));
        assert_ne!(after.newest_write().unwrap(), old);
    }

    /// The speaker moved with a write on its way: the old connection's answer still counts.
    /// Confirmed, the write is done; refused or never answered (the old connection ended), it is
    /// not; meanwhile it is still awaited.
    #[test]
    fn a_write_of_a_moved_speaker_counts_by_the_old_connections_answer() {
        let mute = path(paths::MUTE);
        let mut speaker = state();
        speaker.apply(Event::Connected(identity("left", "KH 120 II")), 0.0);
        speaker.record_written(&mute, &json!(true));
        let answered = speaker.newest_write().unwrap();
        speaker.record_written(&mute, &json!(false));
        let unanswered = speaker.newest_write().unwrap();
        let old = speaker.retire_outstanding();
        assert!(speaker.awaits_write(answered));
        assert!(speaker.refused_writes.is_empty());
        let confirmed = Event::Written {
            path: mute.clone(),
            value: json!(true),
        };
        speaker.apply_retired(old, &confirmed);
        assert!(!speaker.awaits_write(answered));
        assert!(speaker.awaits_write(unanswered));
        speaker.end_retired(old);
        assert_eq!(speaker.refused_writes, vec![unanswered]);
        assert!(!speaker.awaits_write(unanswered));
    }

    /// The new connection read the path before the old one's write reached the speaker: the
    /// confirmed old write has the path read again, so the next gesture does not start from the
    /// read before it. So does one that failed with the old connection, as it may have been
    /// applied; one the speaker refused or did not find changed nothing and needs no read.
    #[test]
    fn an_old_connections_write_has_the_path_read_again() {
        let mute = path(paths::MUTE);
        let mut speaker = state();
        speaker.apply(Event::Connected(identity("left", "KH 120 II")), 0.0);
        for value in [true, false, true, false] {
            speaker.record_written(&mute, &json!(value));
        }
        let old = speaker.retire_outstanding();
        let read_before = Event::Value {
            path: mute.clone(),
            value: json!(false),
        };
        speaker.apply(read_before, 0.0);
        let confirmed = Event::Written {
            path: mute.clone(),
            value: json!(true),
        };
        assert_eq!(
            speaker.apply_retired(old, &confirmed),
            Some(Command::Read(mute.clone()))
        );
        let failed = Event::WriteFailed {
            path: mute.clone(),
            message: String::new(),
        };
        assert_eq!(
            speaker.apply_retired(old, &failed),
            Some(Command::Read(mute.clone()))
        );
        let refused = Event::WriteRejected {
            path: mute.clone(),
            error: DeviceError {
                code: DeviceError::READ_ONLY,
                description: String::new(),
            },
        };
        assert_eq!(speaker.apply_retired(old, &refused), None);
        let absent = Event::WriteAbsent(mute.clone());
        assert_eq!(speaker.apply_retired(old, &absent), None);
    }

    /// Band 1 refused while band 2 of the same list is still on its way: the list waiting for
    /// band 2's answer no longer holds the refused band 1, so a new change of band 1 starts
    /// from what the speaker holds.
    #[test]
    fn a_refused_place_leaves_the_value_still_on_its_way() {
        let gain = path("audio/out/eq2/gain");
        let mut speaker = state();
        speaker.apply(Event::Connected(identity("left", "KH 120 II")), 0.0);
        speaker.apply(value_event("audio/out/eq2/gain", json!([0, 0, 0])), 0.0);
        speaker.record_written(&gain, &json!([1, null, null]));
        speaker.record_written(&gain, &json!([null, 2, null]));
        let rejected = Event::WriteRejected {
            path: gain.clone(),
            error: DeviceError {
                code: DeviceError::READ_ONLY,
                description: String::new(),
            },
        };
        speaker.apply(rejected, 0.0);
        assert_eq!(speaker.latest(&gain), Some(&json!([0, 2, 0])));
        speaker.record_written(&gain, &json!([3, null, null]));
        let newest = speaker.written.last().unwrap();
        assert_eq!(newest.before, Some(json!([0, null, null])));
    }

    /// Band 1 answered and band 2 refused while band 3 is still on its way: the list waiting for
    /// band 3 keeps band 1 as the speaker confirmed it (here clamped to 1.5), or as sent when
    /// its answer told nothing, and drops only the refused band 2. A band sent after band 1's
    /// answer keeps it too.
    #[test]
    fn a_refusal_keeps_the_answered_places_still_on_their_way() {
        let gain = path("audio/out/eq2/gain");
        let rejected = Event::WriteRejected {
            path: gain.clone(),
            error: DeviceError {
                code: DeviceError::READ_ONLY,
                description: String::new(),
            },
        };
        let confirmed = Event::Written {
            path: gain.clone(),
            value: json!([1.5, 0, 0]),
        };
        let failed = Event::WriteFailed {
            path: gain.clone(),
            message: String::new(),
        };
        let cases = [(confirmed, json!([1.5, 0, 3])), (failed, json!([1, 0, 3]))];
        for (first, expected) in cases {
            let mut speaker = state();
            speaker.apply(Event::Connected(identity("left", "KH 120 II")), 0.0);
            speaker.apply(value_event("audio/out/eq2/gain", json!([0, 0, 0])), 0.0);
            speaker.record_written(&gain, &json!([1, null, null]));
            speaker.record_written(&gain, &json!([null, 2, null]));
            speaker.apply(first.clone(), 0.0);
            speaker.record_written(&gain, &json!([null, null, 3]));
            speaker.apply(rejected.clone(), 0.0);
            assert_eq!(speaker.latest(&gain), Some(&expected), "{first:?}");
        }
    }

    /// A refusal names the writes sent after it to the same path, also from an old connection;
    /// a write that failed with the connection is no refusal, it may have been applied.
    #[test]
    fn a_refused_write_names_the_writes_that_followed_it() {
        let mute = path(paths::MUTE);
        let rejected = Event::WriteRejected {
            path: mute.clone(),
            error: DeviceError {
                code: DeviceError::READ_ONLY,
                description: String::new(),
            },
        };
        let failed = Event::WriteFailed {
            path: mute.clone(),
            message: String::new(),
        };
        for moved in [false, true] {
            let mut speaker = state();
            speaker.apply(Event::Connected(identity("left", "KH 120 II")), 0.0);
            let mut writes = Vec::new();
            for value in [true, false, true] {
                speaker.record_written(&mute, &json!(value));
                writes.push(speaker.newest_write().unwrap());
            }
            let old = moved.then(|| speaker.retire_outstanding());
            let answer = |speaker: &mut SpeakerState, event: &Event| match old {
                Some(old) => {
                    speaker.apply_retired(old, event);
                }
                None => speaker.apply(event.clone(), 0.0),
            };
            answer(&mut speaker, &failed);
            assert!(speaker.refused_predecessors.is_empty(), "moved: {moved}");
            answer(&mut speaker, &rejected);
            let expected = RefusedPredecessor {
                write: writes[1],
                later: vec![writes[2]],
            };
            assert_eq!(
                speaker.refused_predecessors,
                vec![expected],
                "moved: {moved}"
            );
        }
    }

    /// Moved twice before the first old worker answered: each old connection's answers count
    /// for its own writes, and each ends on its own.
    #[test]
    fn two_moves_keep_each_old_connections_writes_apart() {
        let mute = path(paths::MUTE);
        let mut speaker = state();
        speaker.apply(Event::Connected(identity("left", "KH 120 II")), 0.0);
        speaker.record_written(&mute, &json!(true));
        let first = speaker.newest_write().unwrap();
        let first_worker = speaker.retire_outstanding();
        speaker.record_written(&mute, &json!(false));
        let second = speaker.newest_write().unwrap();
        let second_worker = speaker.retire_outstanding();
        let refused = Event::WriteFailed {
            path: mute.clone(),
            message: "timeout".to_owned(),
        };
        speaker.apply_retired(second_worker, &refused);
        assert_eq!(speaker.refused_writes, vec![second]);
        assert!(speaker.awaits_write(first));
        speaker.end_retired(second_worker);
        let confirmed = Event::Written {
            path: mute,
            value: json!(true),
        };
        speaker.apply_retired(first_worker, &confirmed);
        speaker.end_retired(first_worker);
        assert_eq!(speaker.refused_writes, vec![second]);
        assert!(!speaker.awaits_write(first));
    }

    /// A shorter list afterwards (a firmware fault): no meter is left at its last level.
    #[test]
    fn a_shorter_meter_list_drops_the_meters_it_no_longer_reports() {
        let mut speaker = state();
        speaker.apply(value_event("m/in/level", json!([-10.0, -12.0, -3.0])), 0.0);
        speaker.apply(value_event("m/in/level", json!([-10.0, -12.0])), 0.1);
        assert_eq!(speaker.input_meters.len(), 2);
        assert_eq!(speaker.loudest_input(10.0), Some(-10.0));
    }

    #[test]
    fn input_and_output_meters_are_kept_apart() {
        let mut speaker = state();
        speaker.apply(value_event("m/in/level", json!([-10.0])), 0.0);
        speaker.apply(value_event("m/out/level", json!([-20.0, -30.0])), 0.0);
        assert_eq!(speaker.input_meters.len(), 1);
        assert_eq!(speaker.input_meters[0].level(), -10.0);
        assert_eq!(speaker.output_meters.len(), 2);
    }

    #[test]
    fn the_loudest_input_is_the_highest_channel_as_shown() {
        let mut speaker = state();
        assert_eq!(speaker.loudest_input(0.0), None);
        speaker.apply(value_event("m/in/level", json!([-30.0, -12.0])), 0.0);
        speaker.apply(value_event("m/out/level", json!([-3.0])), 0.0);
        // Long after the rise, the meters show the levels themselves.
        assert_eq!(speaker.loudest_input(10.0), Some(-12.0));
    }

    #[test]
    fn unreachable_resets_meters() {
        let mut speaker = state();
        speaker.apply(value_event("m/out/level", json!([-3.0])), 0.0);
        speaker.apply(Event::Unreachable("masked".to_owned()), 0.5);
        assert_eq!(speaker.output_meters[0].level(), METER_FLOOR_DB);
        assert_eq!(speaker.output_meters[0].peak(0.5), METER_FLOOR_DB);
    }

    #[test]
    fn a_rejection_message_survives_the_reread_of_its_path() {
        let mut speaker = state();
        speaker.apply(
            Event::WriteRejected {
                path: path("ui/bass_gain"),
                error: DeviceError {
                    code: DeviceError::READ_ONLY,
                    description: String::new(),
                },
            },
            0.0,
        );
        speaker.apply(value_event("ui/bass_gain", json!(0.0)), 0.1);
        speaker.apply(value_event("ui/bass_gain", json!(0.0)), 30.0);
        assert_eq!(
            speaker.message,
            Some(Message::Rejected(DeviceError::READ_ONLY))
        );
    }

    #[test]
    fn a_failed_write_message_survives_values_of_its_path() {
        let mut speaker = state();
        speaker.apply(
            Event::WriteFailed {
                path: path("audio/out/level"),
                message: "lost".to_owned(),
            },
            0.0,
        );
        speaker.apply(value_event("audio/out/level", json!(80.0)), 2.0);
        assert_eq!(speaker.message, Some(Message::WriteFailed));
    }

    #[test]
    fn a_local_mode_rejection_names_the_rear_switch() {
        let mut speaker = state();
        speaker.apply(
            Event::WriteRejected {
                path: path("audio/out/level"),
                error: DeviceError {
                    code: DeviceError::LOCAL_MODE,
                    description: "authorisation needed".to_owned(),
                },
            },
            0.0,
        );
        assert_eq!(
            speaker.message,
            Some(Message::Rejected(DeviceError::LOCAL_MODE))
        );
    }

    #[test]
    fn the_answer_to_the_level_write_ends_the_held_fader_value() {
        let mut speaker = state();
        let level = path(paths::OUTPUT_LEVEL);
        speaker.fader.sent(90.0, 0.0);
        speaker.record_written(&level, &json!(90.0));
        speaker.apply(value_event("device/name", json!("left")), 0.5);
        assert_eq!(speaker.fader.shown(80.0, 0.5), 90.0);
        let answer = Event::Written {
            path: level,
            value: json!(80.0),
        };
        speaker.apply(answer, 0.5);
        assert_eq!(speaker.fader.shown(80.0, 0.5), 80.0);
    }

    #[test]
    fn all_values_go_from_loading_to_loaded_or_failed() {
        let mut speaker = state();
        assert_eq!(speaker.read_all_values(), Command::ReadAll);
        assert_eq!(speaker.all_values, AllValuesState::Loading);
        let reports = vec![LeafReport {
            path: path("device/name"),
            value: Ok(json!("left")),
            limits: None,
        }];
        speaker.apply(Event::AllValues(reports.clone()), 0.0);
        assert_eq!(speaker.all_values, AllValuesState::Loaded(reports));
        speaker.read_all_values();
        speaker.apply(Event::AllValuesFailed("refused".to_owned()), 1.0);
        assert_eq!(
            speaker.all_values,
            AllValuesState::Failed("refused".to_owned())
        );
    }

    #[test]
    fn an_answer_after_closing_does_not_reopen_the_window() {
        let mut speaker = state();
        speaker.read_all_values();
        speaker.all_values_filter = "standby".to_owned();
        speaker.close_all_values();
        speaker.apply(Event::AllValues(Vec::new()), 0.0);
        assert_eq!(speaker.all_values, AllValuesState::Idle);
        assert!(speaker.all_values_filter.is_empty());
    }

    fn identity(name: &str, product: &str) -> Identity {
        Identity {
            product: product.to_owned(),
            version: "1".to_owned(),
            serial: SERIAL.to_owned(),
            name: name.to_owned(),
        }
    }

    #[test]
    fn display_name_falls_back_from_own_to_announced_to_model_to_masked_address() {
        let mut speaker = state();
        assert_eq!(speaker.display_name(), mask_host(&address(ADDRESS_TEXT)));
        speaker.discovered = Some(Found {
            name: String::new(),
            product: Some("KH 750".to_owned()),
            ..found(ADDRESS_TEXT, None)
        });
        assert_eq!(speaker.display_name(), "KH 750");
        speaker.discovered.as_mut().unwrap().name = "announced".to_owned();
        assert_eq!(speaker.display_name(), "announced");
        speaker.identity = Some(identity("", "KH 120 II"));
        assert_eq!(speaker.display_name(), "announced");
        speaker.identity = Some(identity("own", "KH 120 II"));
        assert_eq!(speaker.display_name(), "own");
    }

    #[test]
    fn model_prefers_the_reported_product() {
        let mut speaker = state();
        assert_eq!(speaker.model(), None);
        speaker.discovered = Some(Found {
            product: Some("KH 750".to_owned()),
            ..found(ADDRESS_TEXT, None)
        });
        assert_eq!(speaker.model(), Some("KH 750"));
        speaker.identity = Some(identity("", ""));
        assert_eq!(speaker.model(), Some("KH 750"));
        speaker.identity = Some(identity("", "KH 120 II"));
        assert_eq!(speaker.model(), Some("KH 120 II"));
    }

    /// A name that holds the serial is shown masked, as the announced one is.
    #[test]
    fn the_own_name_shows_its_serial_masked() {
        let address = address(ADDRESS_TEXT);
        let mut speaker = SpeakerState::new(SpeakerKey::Address(address), address);
        let identity = Identity {
            product: "KH 120 II".to_owned(),
            version: "1".to_owned(),
            serial: "3712345678".to_owned(),
            name: "KH 120 II 3712345678".to_owned(),
        };
        speaker.apply(Event::Connected(identity), 0.0);
        assert!(!speaker.display_name().contains("3712345678"));
    }

    /// A worker replaced for a moved speaker answers nothing the old one was asked: the writes
    /// it held no longer hold the path, and a running read can be asked again.
    #[test]
    fn a_replaced_worker_leaves_nothing_outstanding() {
        let mut speaker = state();
        let path = paths::path(paths::MUTE);
        speaker.apply(
            Event::Value {
                path: path.clone(),
                value: json!(false),
            },
            0.0,
        );
        speaker.record_written(&path, &json!(true));
        speaker.all_values = AllValuesState::Loading;
        speaker.full_scan = FullScanState::Running;
        speaker.forget_outstanding();
        assert_eq!(speaker.latest(&path), Some(&json!(false)));
        assert_eq!(speaker.all_values, AllValuesState::Idle);
        assert_eq!(speaker.full_scan, FullScanState::Idle);
    }

    /// The walk's own reason is English and may name an address; the panel words the failure
    /// itself, and the reason goes to the log.
    #[test]
    fn a_failed_full_scan_keeps_no_raw_reason() {
        let mut speaker = state();
        speaker.full_scan = FullScanState::Running;
        speaker.apply(Event::AllValuesFailed("schema has no osc".to_owned()), 1.0);
        assert_eq!(
            speaker.full_scan,
            FullScanState::Failed(ScanFailure::Reading)
        );
    }
}
