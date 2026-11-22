//! A simulated Neumann KH speaker for tests: it answers over TCP exactly like the recorded
//! real devices and can be told to misbehave on the next reply.

mod device;

use std::collections::{HashMap, VecDeque};
use std::io::{self, BufRead, BufReader, Write};
use std::net::{Ipv6Addr, Shutdown, SocketAddr, SocketAddrV6, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::{json, Map, Value};

use crate::ssc::client::{is_timeout, LINE_TERMINATOR, MAX_LINE_BYTES};
use crate::ssc::wire::parse_line;
use device::{encode_line, SimulatedDevice};

/// `Fault::ForeignXidFirst` answers first with the request's xid plus this.
pub const FOREIGN_XID_OFFSET: u64 = 1000;
/// The simulators' ports: below every system's dynamic range (Linux from 32768, Windows from
/// 49152). On port 0 the system handed a stopped simulator's port to the next one started (Linux
/// soon), and a test that expected it unreachable, or restarted it there, met another.
const SIMULATOR_PORTS: std::ops::Range<u16> = 20_000..32_000;
/// The ports of one test process: each holds a range of its own for its life (nextest runs
/// every test in a process of its own; see `process_start`).
const PORTS_PER_PROCESS: u16 = 100;
/// nextest's number for each test that runs at the same time, from 0; unset under `cargo test`.
const NEXTEST_SLOT: &str = "NEXTEST_TEST_GLOBAL_SLOT";
static SIMULATORS_STARTED: AtomicU16 = AtomicU16::new(0);
const FILLER_BYTE: char = 'a';
// Windows does not wake a blocked read when another handle of the socket is shut down, so
// connection threads wake up this often to look for the stop flag.
const READ_POLL_INTERVAL: Duration = Duration::from_millis(50);
// A refused loopback connect takes about 2 s on Windows; waking a finished listener must not.
const WAKE_TIMEOUT: Duration = Duration::from_millis(200);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimulatedModel {
    Kh120Ii,
    Kh750,
}

/// A misbehaviour applied to the next reply only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// An earlier reply without the request's id arrives before the real one.
    StaleLineFirst,
    /// A reply with a foreign id (request id + 1000) arrives before the real one.
    ForeignXidFirst,
    /// The reply leaves out the transaction id.
    NoXidEcho,
    /// A line longer than the 1 MiB limit replaces the reply.
    OverlongLine,
    /// The connection is closed instead of answering.
    CloseConnection,
    /// The request is read and never answered.
    Silence,
    /// The reply carries the request's id and nothing else: an answer that tells nothing.
    EmptyReply,
}

/// What the connection sends back for one request.
enum Delivery {
    Lines(Vec<String>),
    Close,
    Silent,
}

struct State {
    device: SimulatedDevice,
    faults: VecDeque<Fault>,
    /// Faults waiting for a write to their JSON pointer, whatever is polled meanwhile.
    write_faults: Vec<(String, Fault)>,
    requests: Vec<Value>,
    reply_delay: Duration,
}

#[derive(Default)]
struct Connections {
    next_id: u64,
    streams: HashMap<u64, TcpStream>,
    workers: Vec<JoinHandle<()>>,
}

struct Shared {
    state: Mutex<State>,
    connections: Mutex<Connections>,
    stopping: AtomicBool,
}

/// A read names its path with `null`, a write with the value.
fn writes_to(request: &Map<String, Value>, pointer: &str) -> bool {
    let request = Value::Object(request.clone());
    let value = request.pointer(pointer);
    value.is_some_and(|value| !value.is_null())
}

impl Shared {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().expect("simulator state lock")
    }

    fn connections(&self) -> MutexGuard<'_, Connections> {
        self.connections.lock().expect("simulator connection lock")
    }

    /// Answers one request. A fault still applies the request to the tree: the device
    /// received it, only its reply goes wrong.
    fn respond(&self, request: Map<String, Value>) -> Delivery {
        let mut state = self.state();
        state.requests.push(Value::Object(request.clone()));
        let waiting = state
            .write_faults
            .iter()
            .position(|(pointer, _)| writes_to(&request, pointer));
        let fault = match waiting {
            Some(place) => Some(state.write_faults.remove(place).1),
            None => state.faults.pop_front(),
        };
        let answer = state.device.answer(&request);
        let reply = encode_line(&answer.body, answer.xid.as_ref());
        match fault {
            None => Delivery::Lines(vec![reply]),
            Some(Fault::CloseConnection) => Delivery::Close,
            Some(Fault::Silence) => Delivery::Silent,
            Some(Fault::OverlongLine) => Delivery::Lines(vec![overlong_line()]),
            Some(Fault::NoXidEcho) => Delivery::Lines(vec![encode_line(&answer.body, None)]),
            Some(Fault::EmptyReply) => {
                Delivery::Lines(vec![encode_line(&json!({}), answer.xid.as_ref())])
            }
            Some(Fault::ForeignXidFirst) => {
                let foreign =
                    answer.xid.as_ref().and_then(Value::as_u64).unwrap_or(0) + FOREIGN_XID_OFFSET;
                let early = encode_line(&answer.body, Some(&json!(foreign)));
                Delivery::Lines(vec![early, reply])
            }
            Some(Fault::StaleLineFirst) => {
                let earlier = json!({"audio": {"out": {"level": null}}});
                let stale = state.device.answer(earlier.as_object().expect("an object"));
                Delivery::Lines(vec![encode_line(&stale.body, None), reply])
            }
        }
    }

    fn converse(&self, stream: TcpStream) {
        let _ = stream.set_read_timeout(Some(READ_POLL_INTERVAL));
        let Ok(read_half) = stream.try_clone() else {
            return;
        };
        let mut reader = BufReader::new(read_half);
        let mut writer = &stream;
        let mut line = Vec::new();
        loop {
            match reader.read_until(b'\n', &mut line) {
                Ok(0) => break,
                Ok(_) => {}
                Err(error) if is_timeout(&error) => {
                    if self.stopping.load(Ordering::SeqCst) {
                        break;
                    }
                    continue;
                }
                Err(_) => break,
            }
            let parsed = parse_line(line.trim_ascii());
            line.clear();
            let Ok(Value::Object(request)) = parsed else {
                continue;
            };
            let delivery = self.respond(request);
            // Read first: the guard of `self.state()` would otherwise be held through the sleep
            // and stall every other connection and test query for the whole delay.
            let delay = self.state().reply_delay;
            thread::sleep(delay);
            let lines = match delivery {
                Delivery::Lines(lines) => lines,
                Delivery::Silent => continue,
                Delivery::Close => break,
            };
            let payload: String = lines
                .iter()
                .map(|text| format!("{text}{LINE_TERMINATOR}"))
                .collect();
            if writer.write_all(payload.as_bytes()).is_err() {
                break;
            }
        }
        // The registry keeps a duplicate handle, so dropping this one would not close it.
        let _ = stream.shutdown(Shutdown::Both);
    }

    fn serve(self: &Arc<Self>, stream: TcpStream) {
        let _ = stream.set_nodelay(true);
        let Ok(registered) = stream.try_clone() else {
            return;
        };
        let mut connections = self.connections();
        let id = connections.next_id;
        connections.next_id += 1;
        connections.streams.insert(id, registered);
        let shared = Arc::clone(self);
        connections.workers.push(thread::spawn(move || {
            shared.converse(stream);
            shared.connections().streams.remove(&id);
        }));
    }
}

fn overlong_line() -> String {
    FILLER_BYTE.to_string().repeat(MAX_LINE_BYTES + 1)
}

fn accept_loop(shared: Arc<Shared>, listener: TcpListener) {
    for incoming in listener.incoming() {
        if shared.stopping.load(Ordering::SeqCst) {
            break;
        }
        if let Ok(stream) = incoming {
            shared.serve(stream);
        }
    }
}

pub struct Simulator {
    shared: Arc<Shared>,
    address: SocketAddrV6,
    acceptor: Option<JoinHandle<()>>,
}

impl Simulator {
    /// Starts on a port of the IPv6 loopback address that no other simulator of this process
    /// gets, also once this one is stopped: a test can restart it there with `start_on`.
    pub fn start(model: SimulatedModel) -> Simulator {
        let span = SIMULATOR_PORTS.end - SIMULATOR_PORTS.start;
        let slot = std::env::var(NEXTEST_SLOT).ok();
        let process_start = process_start(slot.as_deref(), std::process::id());
        for _ in 0..span {
            let started = SIMULATORS_STARTED.fetch_add(1, Ordering::Relaxed);
            let port = SIMULATOR_PORTS.start + process_start.wrapping_add(started) % span;
            let address = SocketAddrV6::new(Ipv6Addr::LOCALHOST, port, 0, 0);
            // Taken by another process, or barred by the system (Windows excludes port ranges).
            if let Ok(simulator) = Simulator::start_on(model, address) {
                return simulator;
            }
        }
        panic!("no simulator port is free on the IPv6 loopback address");
    }

    /// Starts on `address`, e.g. the one a stopped simulator had, to model a restart.
    pub fn start_on(model: SimulatedModel, address: SocketAddrV6) -> io::Result<Simulator> {
        let listener = TcpListener::bind(address)?;
        let SocketAddr::V6(bound) = listener.local_addr()? else {
            return Err(io::Error::other("listener is not IPv6"));
        };
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                device: SimulatedDevice::new(model),
                faults: VecDeque::new(),
                write_faults: Vec::new(),
                requests: Vec::new(),
                reply_delay: Duration::ZERO,
            }),
            connections: Mutex::new(Connections::default()),
            stopping: AtomicBool::new(false),
        });
        let acceptor = thread::spawn({
            let shared = Arc::clone(&shared);
            move || accept_loop(shared, listener)
        });
        Ok(Simulator {
            shared,
            address: bound,
            acceptor: Some(acceptor),
        })
    }

    pub fn address(&self) -> SocketAddrV6 {
        self.address
    }

    /// Closes the listener and every open connection; the port is free again on return.
    pub fn stop(self) {
        drop(self);
    }

    /// Queues a fault for the next reply; several queued faults apply one per reply.
    pub fn inject(&self, fault: Fault) {
        self.shared.state().faults.push_back(fault);
    }

    /// Queues a fault for the next write to the JSON pointer `pointer`; a poll in between does
    /// not take it, as `inject`'s fault would.
    pub fn inject_on_write(&self, pointer: &str, fault: Fault) {
        let fault = (pointer.to_owned(), fault);
        self.shared.state().write_faults.push(fault);
    }

    /// Every request received so far, parsed.
    pub fn requests(&self) -> Vec<Value> {
        self.shared.state().requests.clone()
    }

    /// Models a slow device: every later reply waits this long.
    pub fn set_reply_delay(&self, delay: Duration) {
        self.shared.state().reply_delay = delay;
    }

    pub fn value(&self, path: &str) -> Option<Value> {
        self.shared.state().device.value(path)
    }

    /// Changes the tree behind the client's back, e.g. to model a knob turned on the speaker.
    pub fn set_value(&self, path: &str, value: Value) {
        self.shared.state().device.set_value(path, value);
    }

    pub fn remove_value(&self, path: &str) {
        self.shared.state().device.remove_value(path);
    }

    /// From now on `osc/schema` of the container `path` is refused with a device error.
    pub fn refuse_schema(&self, path: &str) {
        self.shared.state().device.refuse_schema(path);
    }
}

impl Drop for Simulator {
    fn drop(&mut self) {
        self.shared.stopping.store(true, Ordering::SeqCst);
        // accept() blocks until a connection arrives, so one is made to let the loop see the flag.
        let _ = TcpStream::connect_timeout(&SocketAddr::V6(self.address), WAKE_TIMEOUT);
        if let Some(acceptor) = self.acceptor.take() {
            let _ = acceptor.join();
        }
        let workers = {
            let mut connections = self.shared.connections();
            for stream in connections.streams.values() {
                let _ = stream.shutdown(Shutdown::Both);
            }
            std::mem::take(&mut connections.workers)
        };
        for worker in workers {
            let _ = worker.join();
        }
    }
}

/// Where this process's simulator ports start, as an offset into `SIMULATOR_PORTS`: from
/// nextest's slot (under `cargo test` the process number) on, the first range no other process
/// holds, kept for the life of the process.
fn process_start(slot: Option<&str>, process_id: u32) -> u16 {
    static CLAIMED: OnceLock<(u16, TcpListener)> = OnceLock::new();
    let position = slot
        .and_then(|slot| slot.parse::<u32>().ok())
        .unwrap_or(process_id);
    CLAIMED.get_or_init(|| claim_range(position)).0
}

/// The first range from `preferred` on whose last port can be bound, and that port's listener:
/// while it is held, no other process takes the range. Slots alone repeat in every nextest run
/// (`cargo mutants -j 4` starts four), process numbers repeat modulo the ranges.
fn claim_range(preferred: u32) -> (u16, TcpListener) {
    let processes = u32::from((SIMULATOR_PORTS.end - SIMULATOR_PORTS.start) / PORTS_PER_PROCESS);
    for step in 0..processes {
        let start = ((preferred + step) % processes) as u16 * PORTS_PER_PROCESS;
        let guard = SIMULATOR_PORTS.start + start + PORTS_PER_PROCESS - 1;
        if let Ok(listener) = TcpListener::bind(SocketAddrV6::new(Ipv6Addr::LOCALHOST, guard, 0, 0))
        {
            return (start, listener);
        }
    }
    panic!("no simulator port range is free on the IPv6 loopback address");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// nextest's slots start at 0 in every run, so the runs of `cargo mutants -j 4` shared
    /// ranges; process numbers repeat too. A range is held by one process at a time.
    #[test]
    fn a_range_another_process_holds_is_not_taken() {
        const PREFERRED: u32 = 7;
        let (first, _held) = claim_range(PREFERRED);
        let (second, _also_held) = claim_range(PREFERRED);
        assert_ne!(first, second);
    }
}
