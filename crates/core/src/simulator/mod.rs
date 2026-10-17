//! A simulated Neumann KH speaker for tests: it answers over TCP exactly like the recorded
//! real devices and can be told to misbehave on the next reply.

mod device;

use std::collections::{HashMap, VecDeque};
use std::io::{self, BufRead, BufReader, Write};
use std::net::{Ipv6Addr, Shutdown, SocketAddr, SocketAddrV6, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::{json, Map, Value};

use crate::ssc::wire::parse_line;
use device::{encode_line, SimulatedDevice};

const LINE_TERMINATOR: &str = "\r\n";
const LINE_LIMIT_BYTES: usize = 1_048_576;
const FOREIGN_XID_OFFSET: u64 = 1000;
const FILLER_BYTE: char = 'a';
// Windows does not wake a blocked read when another handle of the socket is shut down, so
// connection threads wake up this often to look for the stop flag.
const READ_POLL_INTERVAL: Duration = Duration::from_millis(50);
// A refused loopback connect takes about 2 s on Windows; waking a finished listener must not.
const WAKE_TIMEOUT: Duration = Duration::from_millis(200);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimModel {
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
        let fault = state.faults.pop_front();
        let answer = state.device.answer(&request);
        let reply = encode_line(&answer.body, answer.xid.as_ref());
        match fault {
            None => Delivery::Lines(vec![reply]),
            Some(Fault::CloseConnection) => Delivery::Close,
            Some(Fault::Silence) => Delivery::Silent,
            Some(Fault::OverlongLine) => Delivery::Lines(vec![overlong_line()]),
            Some(Fault::NoXidEcho) => Delivery::Lines(vec![encode_line(&answer.body, None)]),
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
                Err(error) if is_poll_timeout(&error) => {
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
            thread::sleep(self.state().reply_delay);
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

fn is_poll_timeout(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

fn overlong_line() -> String {
    FILLER_BYTE.to_string().repeat(LINE_LIMIT_BYTES + 1)
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
    /// Starts on a free port of the IPv6 loopback address.
    pub fn start(model: SimModel) -> Simulator {
        let any_port = SocketAddrV6::new(Ipv6Addr::LOCALHOST, 0, 0, 0);
        Simulator::start_on(model, any_port).expect("IPv6 loopback is available")
    }

    /// Starts on `address`, e.g. the one a stopped simulator had, to model a restart.
    pub fn start_on(model: SimModel, address: SocketAddrV6) -> io::Result<Simulator> {
        let listener = TcpListener::bind(address)?;
        let SocketAddr::V6(bound) = listener.local_addr()? else {
            return Err(io::Error::other("listener is not IPv6"));
        };
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                device: SimulatedDevice::new(model),
                faults: VecDeque::new(),
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
