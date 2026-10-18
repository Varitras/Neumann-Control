//! Blocking SSC v1 client: one TCP connection to one speaker, one request in flight.
//!
//! Replies are matched by the transaction id the client sends with every request. The reader
//! enforces every limit while reading, because the peer is a network device that can send
//! anything; after any breach the connection is closed and the client is single-use.

use std::fmt;
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, SocketAddrV6, TcpStream};
use std::time::{Duration, Instant};

use serde_json::{Map, Value};

use crate::mask::mask_host;
use crate::ssc::wire::{
    device_error, extract, number, parse_line, reply_xid, request_line, DeviceError, SscPath,
};

pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
pub const RESPONSE_TIMEOUT: Duration = Duration::from_secs(3);
pub const MAX_LINE_BYTES: usize = 1_048_576;
pub const MAX_RESPONSE_LINES: usize = 256;
pub const MAX_RESPONSE_BYTES: usize = 4 * MAX_LINE_BYTES;
pub const MAX_READ_DURATION: Duration = Duration::from_secs(10);

const READ_CHUNK_BYTES: usize = 8192;
const LINE_TERMINATOR: &str = "\r\n";
const FIRST_XID: u64 = 1;
const SCHEMA_PATH: &str = "osc/schema";
const LIMITS_PATH: &str = "osc/limits";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Container,
    Leaf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SscError {
    Connection(String),
    Timeout,
    Device(DeviceError),
    /// The speaker answered this request, but without a usable value (missing, `null`, wrong
    /// shape). The stream is still in step, so the connection stays.
    Unusable(String),
    Protocol(String),
}

impl SscError {
    /// Whether the connection is unusable afterwards and the caller has to reconnect.
    pub fn is_connection_lost(&self) -> bool {
        !matches!(self, SscError::Device(_) | SscError::Unusable(_))
    }
}

impl fmt::Display for SscError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SscError::Connection(message) => write!(formatter, "connection error: {message}"),
            SscError::Timeout => formatter.write_str("the speaker did not answer in time"),
            SscError::Device(error) => write!(
                formatter,
                "the speaker refused the request ({}): {}",
                error.code, error.description
            ),
            SscError::Unusable(message) => write!(formatter, "unusable answer: {message}"),
            SscError::Protocol(message) => write!(formatter, "protocol error: {message}"),
        }
    }
}

impl std::error::Error for SscError {}

/// The socket plus the bytes read from it but not yet handed out as lines.
struct Connection {
    stream: TcpStream,
    host: String,
    pending: Vec<u8>,
    scanned: usize,
}

impl Connection {
    fn send_line(&mut self, line: &str) -> Result<(), SscError> {
        let message = format!("{line}{LINE_TERMINATOR}");
        self.stream
            .set_write_timeout(Some(RESPONSE_TIMEOUT))
            .and_then(|()| self.stream.write_all(message.as_bytes()))
            .map_err(|error| self.connection_error("write to", &error))
    }

    /// The next line without its terminator. Never buffers more than one line limit.
    fn next_line(&mut self, deadline: Instant) -> Result<Vec<u8>, SscError> {
        loop {
            if let Some(line) = self.take_line()? {
                return Ok(line);
            }
            if self.pending.len() > MAX_LINE_BYTES + 1 {
                return Err(self.line_too_long());
            }
            self.fill(deadline)?;
        }
    }

    fn take_line(&mut self) -> Result<Option<Vec<u8>>, SscError> {
        let unscanned = &self.pending[self.scanned..];
        let Some(offset) = unscanned.iter().position(|&byte| byte == b'\n') else {
            self.scanned = self.pending.len();
            return Ok(None);
        };
        let end = self.scanned + offset;
        let mut line: Vec<u8> = self.pending.drain(..=end).collect();
        self.scanned = 0;
        line.pop();
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        if line.len() > MAX_LINE_BYTES {
            return Err(self.line_too_long());
        }
        Ok(Some(line))
    }

    fn fill(&mut self, deadline: Instant) -> Result<(), SscError> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(self.read_duration_exceeded());
        }
        let wait = remaining.min(RESPONSE_TIMEOUT);
        let mut chunk = [0u8; READ_CHUNK_BYTES];
        let outcome = self
            .stream
            .set_read_timeout(Some(wait))
            .and_then(|()| self.stream.read(&mut chunk));
        match outcome {
            Ok(0) => Err(SscError::Connection(format!(
                "{} closed the connection",
                self.host
            ))),
            Ok(count) => {
                self.pending.extend_from_slice(&chunk[..count]);
                Ok(())
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => Ok(()),
            // A wait shortened to the remaining budget is the read-duration limit, not silence.
            Err(error) if is_timeout(&error) && wait < RESPONSE_TIMEOUT => {
                Err(self.read_duration_exceeded())
            }
            Err(error) if is_timeout(&error) => Err(SscError::Timeout),
            Err(error) => Err(self.connection_error("read from", &error)),
        }
    }

    fn connection_error(&self, action: &str, error: &io::Error) -> SscError {
        SscError::Connection(format!("could not {action} {}: {error}", self.host))
    }

    fn line_too_long(&self) -> SscError {
        SscError::Protocol(format!(
            "{} sent a line over {MAX_LINE_BYTES} bytes",
            self.host
        ))
    }

    fn read_duration_exceeded(&self) -> SscError {
        SscError::Protocol(format!(
            "{} kept sending for more than {} s",
            self.host,
            MAX_READ_DURATION.as_secs()
        ))
    }
}

fn is_timeout(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

/// What to do with a line that arrived while waiting for the reply to one request.
enum Verdict {
    Reply,
    Skip,
}

pub struct Client {
    connection: Connection,
    next_xid: u64,
    closed: bool,
    /// Time from sending the last answered request to its matching reply, until taken.
    round_trip: Option<Duration>,
}

impl Client {
    pub fn connect(address: SocketAddrV6) -> Result<Client, SscError> {
        let host = mask_host(&address);
        let stream = TcpStream::connect_timeout(&SocketAddr::V6(address), CONNECT_TIMEOUT)
            .map_err(|error| match error.kind() {
                io::ErrorKind::TimedOut => SscError::Timeout,
                _ => SscError::Connection(format!("could not connect to {host}: {error}")),
            })?;
        // Requests are single small lines; Nagle's delay would add latency to each of them.
        let _ = stream.set_nodelay(true);
        Ok(Client {
            connection: Connection {
                stream,
                host,
                pending: Vec::new(),
                scanned: 0,
            },
            next_xid: FIRST_XID,
            closed: false,
            round_trip: None,
        })
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    pub fn get(&mut self, path: &SscPath) -> Result<Value, SscError> {
        let reply = self.exchange(path, Value::Null)?;
        confirmed_value(&reply, path)
    }

    /// Sets a value and returns the value the device confirms (it may clamp).
    pub fn set(&mut self, path: &SscPath, value: Value) -> Result<Value, SscError> {
        let reply = self.exchange(path, value)?;
        confirmed_value(&reply, path)
    }

    /// Like `set` for a number; NaN and infinities have no JSON form and are never sent.
    pub fn set_number(&mut self, path: &SscPath, value: f64) -> Result<Value, SscError> {
        let value = number(value).map_err(|error| SscError::Protocol(error.to_string()))?;
        self.set(path, value)
    }

    /// The children of `path` (the root for `None`) and whether each is a container.
    pub fn schema(&mut self, path: Option<&SscPath>) -> Result<Vec<(String, NodeKind)>, SscError> {
        let argument = path.map_or(Value::Null, introspection_argument);
        let reply = self.exchange(&introspection_path(SCHEMA_PATH), argument)?;
        match introspected_node(&reply, SCHEMA_PATH, path)? {
            Value::Object(children) => Ok(children
                .iter()
                .map(|(name, child)| (name.clone(), node_kind(child)))
                .collect()),
            Value::Null => Ok(Vec::new()),
            _ => Err(unusable("schema level is not an object")),
        }
    }

    /// The limits dict of one leaf.
    pub fn limits(&mut self, path: &SscPath) -> Result<Value, SscError> {
        let reply = self.exchange(
            &introspection_path(LIMITS_PATH),
            introspection_argument(path),
        )?;
        let node = introspected_node(&reply, LIMITS_PATH, Some(path))?;
        let description = match node {
            Value::Array(items) => items.first(),
            other => Some(other),
        };
        match description {
            Some(found @ Value::Object(_)) => Ok(found.clone()),
            _ => Err(unusable("limits reply holds no description")),
        }
    }

    /// The round trip of the last answered request, once.
    pub fn take_round_trip(&mut self) -> Option<Duration> {
        self.round_trip.take()
    }

    fn close(&mut self) {
        self.closed = true;
        let _ = self.connection.stream.shutdown(Shutdown::Both);
    }

    /// One request and its reply tree; a device error in the reply becomes `Err`.
    fn exchange(&mut self, path: &SscPath, value: Value) -> Result<Value, SscError> {
        if self.closed {
            let host = &self.connection.host;
            return Err(SscError::Connection(format!(
                "connection to {host} is closed"
            )));
        }
        let result = self.transact(path, value);
        if matches!(&result, Err(error) if error.is_connection_lost()) {
            self.close();
        }
        result
    }

    fn transact(&mut self, path: &SscPath, value: Value) -> Result<Value, SscError> {
        let xid = self.next_xid;
        self.next_xid += 1;
        let sent = Instant::now();
        self.connection.send_line(&request_line(path, value, xid))?;
        let reply = self.read_reply(xid, path)?;
        self.round_trip = Some(sent.elapsed());
        match device_error(&reply) {
            Some(error) => Err(SscError::Device(error)),
            None => Ok(reply),
        }
    }

    fn read_reply(&mut self, xid: u64, path: &SscPath) -> Result<Value, SscError> {
        let deadline = Instant::now() + MAX_READ_DURATION;
        let mut lines = 0usize;
        let mut bytes = 0usize;
        loop {
            let line = self.connection.next_line(deadline)?;
            lines += 1;
            bytes += line.len() + LINE_TERMINATOR.len();
            if lines > MAX_RESPONSE_LINES || bytes > MAX_RESPONSE_BYTES {
                return Err(protocol("response exceeded the line or byte budget"));
            }
            if line.is_empty() {
                continue;
            }
            let tree = parse_line(&line).map_err(|error| SscError::Protocol(error.to_string()))?;
            if let Verdict::Reply = judge(&tree, xid, path) {
                return Ok(tree);
            }
        }
    }
}

/// A line with our id is the reply, one with another id is stale; a line without an id counts
/// as the reply only when it answers the requested path or carries an error.
fn judge(tree: &Value, xid: u64, path: &SscPath) -> Verdict {
    match reply_xid(tree) {
        Some(received) if received == xid => Verdict::Reply,
        Some(_) => Verdict::Skip,
        None if extract(tree, path).is_some() || device_error(tree).is_some() => Verdict::Reply,
        None => Verdict::Skip,
    }
}

fn confirmed_value(reply: &Value, path: &SscPath) -> Result<Value, SscError> {
    extract(reply, path)
        .cloned()
        .ok_or_else(|| unusable("reply without the requested path"))
}

fn protocol(message: &str) -> SscError {
    SscError::Protocol(message.to_owned())
}

fn unusable(message: &str) -> SscError {
    SscError::Unusable(message.to_owned())
}

fn introspection_path(text: &str) -> SscPath {
    SscPath::parse(text).expect("constant introspection path is valid")
}

/// `[ {a:{b:null}} ]`: the list holding one address tree that ends in `null`.
fn introspection_argument(path: &SscPath) -> Value {
    let tree = path
        .segments()
        .iter()
        .rev()
        .fold(Value::Null, |inner, segment| {
            Value::Object(Map::from_iter([(segment.clone(), inner)]))
        });
    Value::Array(vec![tree])
}

/// The node an `osc/schema` or `osc/limits` reply describes: the first list entry, followed
/// down `path` (the entry itself for the root).
fn introspected_node<'a>(
    reply: &'a Value,
    method: &str,
    path: Option<&SscPath>,
) -> Result<&'a Value, SscError> {
    let listed = extract(reply, &introspection_path(method))
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .ok_or_else(|| unusable("reply without the requested path"))?;
    let segments = path.map_or(&[][..], SscPath::segments);
    segments
        .iter()
        .try_fold(listed, |node, segment| node.get(segment))
        .ok_or_else(|| unusable("reply does not describe the requested path"))
}

fn node_kind(node: &Value) -> NodeKind {
    match node {
        Value::Object(_) => NodeKind::Container,
        _ => NodeKind::Leaf,
    }
}
