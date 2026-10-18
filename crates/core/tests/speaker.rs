use std::net::{Ipv6Addr, SocketAddrV6, TcpListener};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use neumann_control_core::catalog::Catalog;
use neumann_control_core::schema_walk::{read_all, WalkError, MAX_SCHEMA_DEPTH, MAX_SCHEMA_NODES};
use neumann_control_core::simulator::{Fault, SimModel, Simulator};
use neumann_control_core::speaker::{
    spawn, Command, Event, Events, Identity, SpeakerHandle, Target, Timing,
};
use neumann_control_core::ssc::client::Client;
use neumann_control_core::ssc::wire::SscPath;
use serde_json::{json, Value};

const EVENT_DEADLINE: Duration = Duration::from_secs(5);
// A refused loopback connect takes about 2 s on Windows, so a restart needs a few of them.
const RECOVERY_DEADLINE: Duration = Duration::from_secs(15);
const REQUEST_POLL_INTERVAL: Duration = Duration::from_millis(10);
const QUIET_PERIOD: Duration = Duration::from_millis(300);
const STOP_DEADLINE: Duration = Duration::from_secs(1);
// Slow enough that a walk or connect-time read takes many seconds.
const SLOW_REPLY: Duration = Duration::from_millis(200);
const LONG_INTERVAL: Duration = Duration::from_secs(60);
const READ_ONLY_CODE: i64 = 405;
const KH_120_II_LEAVES: usize = 50;
const KH_120_II_PRODUCT: &str = "KH 120 II";
const KH_120_II_SERIAL: &str = "SIM0001234";

fn path(text: &str) -> SscPath {
    SscPath::parse(text).expect("a valid path")
}

fn short_timing() -> Timing {
    Timing {
        meter: Duration::from_millis(20),
        fast: Duration::from_millis(50),
        slow: Duration::from_millis(200),
        reconnect: Duration::from_millis(100),
        ..Timing::default()
    }
}

fn quiet_timing() -> Timing {
    Timing {
        meter: LONG_INTERVAL,
        fast: LONG_INTERVAL,
        slow: LONG_INTERVAL,
        reconnect: Duration::from_millis(100),
        ..Timing::default()
    }
}

fn no_wake() -> Arc<dyn Fn() + Send + Sync> {
    Arc::new(|| {})
}

fn start_worker(
    address: SocketAddrV6,
    expected_serial: Option<&str>,
    timing: Timing,
) -> (SpeakerHandle, Events) {
    let target = Target {
        address,
        expected_serial: expected_serial.map(str::to_owned),
    };
    spawn(target, timing, no_wake())
}

/// Every event up to and including the first that matches; panics when none comes in time.
fn collect_until(
    events: &Events,
    deadline: Duration,
    matches: impl Fn(&Event) -> bool,
) -> Vec<Event> {
    let end = Instant::now() + deadline;
    let mut seen = Vec::new();
    loop {
        let remaining = end.saturating_duration_since(Instant::now());
        match events.recv_timeout(remaining) {
            Ok(event) => {
                let found = matches(&event);
                seen.push(event);
                if found {
                    return seen;
                }
            }
            Err(error) => panic!("no matching event ({error:?}); seen: {seen:?}"),
        }
    }
}

fn is_connected(event: &Event) -> bool {
    matches!(event, Event::Connected(_))
}

fn is_unreachable(event: &Event) -> bool {
    matches!(event, Event::Unreachable(_))
}

fn is_value_of(event: &Event, wanted: &SscPath) -> bool {
    matches!(event, Event::Value { path, .. } if path == wanted)
}

/// Whether a parsed request names `wanted`, read or write.
fn request_names(request: &Value, wanted: &SscPath) -> bool {
    wanted
        .segments()
        .iter()
        .try_fold(request, |node, segment| node.get(segment))
        .is_some()
}

fn count_requests(simulator: &Simulator, wanted: &SscPath) -> usize {
    simulator
        .requests()
        .iter()
        .filter(|request| request_names(request, wanted))
        .count()
}

fn wait_for_requests(simulator: &Simulator, wanted: &SscPath, count: usize, deadline: Duration) {
    let end = Instant::now() + deadline;
    while count_requests(simulator, wanted) < count {
        assert!(
            Instant::now() < end,
            "fewer than {count} requests for {wanted}"
        );
        thread::sleep(REQUEST_POLL_INTERVAL);
    }
}

/// The last path the worker reads when connecting: once its value arrives, polling has begun.
fn last_initial_path(product: &str) -> SscPath {
    Catalog::embedded()
        .entries_for(product)
        .last()
        .expect("the catalog lists entries")
        .path
        .clone()
}

/// Accepts connections and closes them at once: a peer that is there but never answers a
/// request, so every attempt fails fast.
struct ClosingListener {
    address: SocketAddrV6,
    accepted: Arc<AtomicUsize>,
    stopping: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ClosingListener {
    fn start() -> ClosingListener {
        let listener = TcpListener::bind(SocketAddrV6::new(Ipv6Addr::LOCALHOST, 0, 0, 0))
            .expect("IPv6 loopback is available");
        let std::net::SocketAddr::V6(address) = listener.local_addr().expect("a bound address")
        else {
            panic!("listener is not IPv6");
        };
        let accepted = Arc::new(AtomicUsize::new(0));
        let stopping = Arc::new(AtomicBool::new(false));
        let thread = thread::spawn({
            let accepted = Arc::clone(&accepted);
            let stopping = Arc::clone(&stopping);
            move || {
                for incoming in listener.incoming() {
                    if stopping.load(Ordering::SeqCst) {
                        break;
                    }
                    drop(incoming);
                    accepted.fetch_add(1, Ordering::SeqCst);
                }
            }
        });
        ClosingListener {
            address,
            accepted,
            stopping,
            thread: Some(thread),
        }
    }

    fn wait_for_accepts(&self, count: usize) {
        let end = Instant::now() + EVENT_DEADLINE;
        while self.accepted.load(Ordering::SeqCst) < count {
            assert!(
                Instant::now() < end,
                "fewer than {count} connection attempts"
            );
            thread::sleep(REQUEST_POLL_INTERVAL);
        }
    }
}

impl Drop for ClosingListener {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        let _ = std::net::TcpStream::connect(self.address);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[test]
fn worker_reports_identity_then_values() {
    let simulator = Simulator::start(SimModel::Kh120Ii);
    let woken = Arc::new(AtomicUsize::new(0));
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new({
        let woken = Arc::clone(&woken);
        move || {
            woken.fetch_add(1, Ordering::SeqCst);
        }
    });
    let target = Target {
        address: simulator.address(),
        expected_serial: Some(KH_120_II_SERIAL.to_owned()),
    };
    let (_handle, events) = spawn(target, short_timing(), wake);

    let first = events.recv_timeout(EVENT_DEADLINE).expect("an event");
    assert_eq!(
        first,
        Event::Connected(Identity {
            product: KH_120_II_PRODUCT.to_owned(),
            version: "1_7_3".to_owned(),
            serial: KH_120_II_SERIAL.to_owned(),
            name: "Simulated KH 120 II".to_owned(),
        })
    );
    let level = path("audio/out/level");
    let seen = collect_until(&events, EVENT_DEADLINE, |event| is_value_of(event, &level));
    let expected = Event::Value {
        path: level,
        value: simulator
            .value("audio/out/level")
            .expect("the sim has a level"),
    };
    assert_eq!(seen.last(), Some(&expected));
    // wake runs after the event is sent, so it may trail the receiver by a moment.
    let end = Instant::now() + EVENT_DEADLINE;
    while woken.load(Ordering::SeqCst) < seen.len() + 1 {
        assert!(Instant::now() < end, "wake was not called for every event");
        thread::sleep(REQUEST_POLL_INTERVAL);
    }
}

#[test]
fn set_command_emits_confirmed_value() {
    let simulator = Simulator::start(SimModel::Kh120Ii);
    let (handle, events) = start_worker(simulator.address(), None, short_timing());
    collect_until(&events, EVENT_DEADLINE, is_connected);

    let mute = path("audio/out/mute");
    handle.send(Command::Set {
        path: mute.clone(),
        value: json!(true),
    });
    collect_until(&events, EVENT_DEADLINE, |event| {
        *event
            == Event::Value {
                path: mute.clone(),
                value: json!(true),
            }
    });
    assert_eq!(simulator.value("audio/out/mute"), Some(json!(true)));
}

#[test]
fn read_only_set_emits_rejection_and_reread() {
    let simulator = Simulator::start(SimModel::Kh750);
    let (handle, events) = start_worker(simulator.address(), None, quiet_timing());
    collect_until(&events, EVENT_DEADLINE, is_connected);

    let standby = path("device/standby/enabled");
    let current = simulator
        .value("device/standby/enabled")
        .expect("the sim has the path");
    let refused = Value::Bool(!current.as_bool().expect("a switch"));
    handle.send(Command::Set {
        path: standby.clone(),
        value: refused,
    });
    let seen = collect_until(&events, EVENT_DEADLINE, |event| {
        matches!(event, Event::WriteRejected { .. })
    });
    let Some(Event::WriteRejected { path, error }) = seen.last() else {
        unreachable!("collect_until returns the match last");
    };
    assert_eq!(path, &standby);
    assert_eq!(error.code, READ_ONLY_CODE);

    let reread = events.recv_timeout(EVENT_DEADLINE).expect("the re-read");
    assert_eq!(
        reread,
        Event::Value {
            path: standby,
            value: current,
        }
    );
}

#[test]
fn absent_path_is_reported_once_and_not_polled_again() {
    let simulator = Simulator::start(SimModel::Kh120Ii);
    simulator.remove_value("audio/out/mute");
    let (_handle, events) = start_worker(simulator.address(), None, short_timing());

    let mute = path("audio/out/mute");
    let mut seen = collect_until(&events, EVENT_DEADLINE, |event| {
        *event == Event::Absent(mute.clone())
    });
    // Another fast path read three more times proves several fast rounds went by.
    let level = path("audio/out/level");
    for _ in 0..3 {
        seen.extend(collect_until(&events, EVENT_DEADLINE, |event| {
            is_value_of(event, &level)
        }));
    }

    let absent_reports = seen
        .iter()
        .filter(|event| **event == Event::Absent(mute.clone()))
        .count();
    assert_eq!(absent_reports, 1);
    assert_eq!(count_requests(&simulator, &mute), 1);
}

#[test]
fn null_value_from_the_device_keeps_the_connection() {
    let simulator = Simulator::start(SimModel::Kh120Ii);
    simulator.set_value("audio/out/mute", Value::Null);
    // Only fast paths repeat, so the product is read only when a connection starts.
    let timing = Timing {
        meter: LONG_INTERVAL,
        slow: LONG_INTERVAL,
        ..short_timing()
    };
    let (_handle, events) = start_worker(simulator.address(), None, timing);

    let mute = path("audio/out/mute");
    let level = path("audio/out/level");
    let mut seen = collect_until(&events, EVENT_DEADLINE, is_connected);
    wait_for_requests(&simulator, &mute, 3, EVENT_DEADLINE);
    seen.extend(collect_until(&events, EVENT_DEADLINE, |event| {
        is_value_of(event, &level)
    }));

    assert!(!seen.iter().any(is_unreachable), "seen: {seen:?}");
    assert!(!seen.iter().any(|event| is_value_of(event, &mute)));
    // A connection reads the product twice (identity check, then the catalog's first round);
    // a reconnect would add two more.
    assert_eq!(
        count_requests(&simulator, &path("device/identity/product")),
        2
    );
}

#[test]
fn set_on_absent_path_reports_absent_again() {
    let simulator = Simulator::start(SimModel::Kh120Ii);
    simulator.remove_value("audio/out/mute");
    let (handle, events) = start_worker(simulator.address(), None, quiet_timing());
    let mute = path("audio/out/mute");
    collect_until(&events, EVENT_DEADLINE, |event| {
        *event == Event::Absent(mute.clone())
    });

    handle.send(Command::Set {
        path: mute.clone(),
        value: json!(true),
    });
    collect_until(&events, EVENT_DEADLINE, |event| {
        *event == Event::Absent(mute.clone())
    });
}

#[test]
fn commands_run_before_polling() {
    let simulator = Simulator::start(SimModel::Kh120Ii);
    let busy = Timing {
        meter: LONG_INTERVAL,
        fast: Duration::from_millis(1),
        slow: Duration::from_millis(1),
        reconnect: Duration::from_millis(100),
        ..Timing::default()
    };
    let (handle, events) = start_worker(simulator.address(), None, busy);
    // A fast path read twice means the connect-time read is over and polling runs nonstop.
    let level = path("audio/out/level");
    collect_until(&events, EVENT_DEADLINE, |event| is_value_of(event, &level));
    collect_until(&events, EVENT_DEADLINE, |event| is_value_of(event, &level));

    handle.send(Command::Set {
        path: level.clone(),
        value: json!(90.0),
    });
    let after_send = simulator.requests().len();
    collect_until(&events, EVENT_DEADLINE, |event| {
        *event
            == Event::Value {
                path: level.clone(),
                value: json!(90.0),
            }
    });

    let requests = simulator.requests();
    let set_index = requests
        .iter()
        .position(|request| request["audio"]["out"]["level"] == json!(90.0))
        .expect("the set reached the simulator");
    // At most the one poll that was already under way may come first.
    assert!(
        set_index <= after_send + 1,
        "set was request {set_index}, {after_send} requests when it was sent"
    );
}

#[test]
fn meters_are_polled_between_slow_paths() {
    let simulator = Simulator::start(SimModel::Kh120Ii);
    let busy = Timing {
        meter: Duration::from_millis(50),
        fast: Duration::from_millis(1),
        slow: Duration::from_millis(1),
        reconnect: Duration::from_millis(100),
        ..Timing::default()
    };
    let (_handle, events) = start_worker(simulator.address(), None, busy);
    let last = last_initial_path(KH_120_II_PRODUCT);
    collect_until(&events, EVENT_DEADLINE, |event| is_value_of(event, &last));

    let meter = path("m/in/level");
    let slow = path("device/name");
    let meters_before = count_requests(&simulator, &meter);
    let slow_before = count_requests(&simulator, &slow);
    let end = Instant::now() + Duration::from_secs(1);
    while Instant::now() < end && count_requests(&simulator, &meter) < meters_before + 10 {
        // Read like the app does; unread events would pause polling.
        events.try_iter().for_each(drop);
        thread::sleep(REQUEST_POLL_INTERVAL);
    }

    assert!(count_requests(&simulator, &meter) >= meters_before + 10);
    assert!(count_requests(&simulator, &slow) > slow_before);
}

#[test]
fn worker_recovers_after_simulator_restart() {
    let simulator = Simulator::start(SimModel::Kh120Ii);
    let address = simulator.address();
    let (_handle, events) = start_worker(address, None, short_timing());
    collect_until(&events, EVENT_DEADLINE, is_connected);

    simulator.stop();
    let seen = collect_until(&events, EVENT_DEADLINE, is_unreachable);
    let Some(Event::Unreachable(reason)) = seen.last() else {
        unreachable!("collect_until returns the match last");
    };
    assert!(!reason.contains(&address.ip().to_string()));

    let _restarted = Simulator::start_on(SimModel::Kh120Ii, address).expect("the port is free");
    let seen = collect_until(&events, RECOVERY_DEADLINE, is_connected);
    assert!(!seen.iter().any(is_unreachable), "outage reported twice");
}

#[test]
fn unreachable_is_reported_once_per_outage() {
    let listener = ClosingListener::start();
    let (_handle, events) = start_worker(listener.address, None, short_timing());

    collect_until(&events, EVENT_DEADLINE, is_unreachable);
    listener.wait_for_accepts(3);

    let further: Vec<Event> = events.try_iter().collect();
    assert_eq!(further, Vec::new());
}

#[test]
fn commands_while_unreachable_fail_at_once() {
    let listener = ClosingListener::start();
    let timing = Timing {
        reconnect: LONG_INTERVAL,
        ..short_timing()
    };
    let (handle, events) = start_worker(listener.address, None, timing);
    collect_until(&events, EVENT_DEADLINE, is_unreachable);

    let level = path("audio/out/level");
    handle.send(Command::Set {
        path: level.clone(),
        value: json!(90.0),
    });
    handle.send(Command::ReadAll);
    let failed = events
        .recv_timeout(EVENT_DEADLINE)
        .expect("a write failure");
    assert!(matches!(failed, Event::WriteFailed { path, .. } if path == level));
    let failed = events
        .recv_timeout(EVENT_DEADLINE)
        .expect("a read-all failure");
    assert!(matches!(failed, Event::AllValuesFailed(_)));
}

#[test]
fn connection_loss_during_set_reports_write_failed() {
    let simulator = Simulator::start(SimModel::Kh120Ii);
    let (handle, events) = start_worker(simulator.address(), None, quiet_timing());
    let last = last_initial_path(KH_120_II_PRODUCT);
    collect_until(&events, EVENT_DEADLINE, |event| is_value_of(event, &last));

    simulator.inject(Fault::CloseConnection);
    let mute = path("audio/out/mute");
    handle.send(Command::Set {
        path: mute.clone(),
        value: json!(true),
    });
    let failed = events
        .recv_timeout(EVENT_DEADLINE)
        .expect("a write failure");
    assert!(matches!(failed, Event::WriteFailed { path, .. } if path == mute));
    let lost = events.recv_timeout(EVENT_DEADLINE).expect("the outage");
    assert!(is_unreachable(&lost));
}

#[test]
fn serial_mismatch_is_reported() {
    let simulator = Simulator::start(SimModel::Kh120Ii);
    let (_handle, events) = start_worker(simulator.address(), Some("OTHER"), short_timing());

    let first = events.recv_timeout(EVENT_DEADLINE).expect("an event");
    assert_eq!(
        first,
        Event::SerialMismatch {
            expected: "OTHER".to_owned(),
            found: KH_120_II_SERIAL.to_owned(),
        }
    );
    let serial = path("device/identity/serial");
    wait_for_requests(&simulator, &serial, 2, EVENT_DEADLINE);
    match events.recv_timeout(QUIET_PERIOD) {
        Err(RecvTimeoutError::Timeout) => {}
        other => panic!("a mismatched speaker must stay silent, got {other:?}"),
    }
}

#[test]
fn read_all_lists_every_leaf() {
    let simulator = Simulator::start(SimModel::Kh120Ii);
    let mut client = Client::connect(simulator.address()).expect("simulator accepts");

    let reports = read_all(&mut client, &|| false).expect("the walk succeeds");

    assert_eq!(reports.len(), KH_120_II_LEAVES);
    let mut paths: Vec<String> = reports.iter().map(|leaf| leaf.path.to_string()).collect();
    paths.sort();
    paths.dedup();
    assert_eq!(paths.len(), KH_120_II_LEAVES);
    assert!(paths.iter().all(|text| !text.starts_with("osc/")));
    let level = reports
        .iter()
        .find(|leaf| leaf.path == path("audio/out/level"))
        .expect("the level is a leaf");
    assert_eq!(
        level.value,
        Ok(simulator
            .value("audio/out/level")
            .expect("the sim has a level"))
    );
    assert_eq!(level.limits, Some(json!({"type": "Number"})));
}

#[test]
fn read_all_command_reports_all_values() {
    let simulator = Simulator::start(SimModel::Kh120Ii);
    let (handle, events) = start_worker(simulator.address(), None, short_timing());
    collect_until(&events, EVENT_DEADLINE, is_connected);

    handle.send(Command::ReadAll);
    let seen = collect_until(&events, EVENT_DEADLINE, |event| {
        matches!(event, Event::AllValues(_) | Event::AllValuesFailed(_))
    });
    let Some(Event::AllValues(reports)) = seen.last() else {
        panic!("expected all values, got {:?}", seen.last());
    };
    assert_eq!(reports.len(), KH_120_II_LEAVES);
}

/// Long enough per request that the KH 750 walk (about 260 requests) takes over a second.
const WALK_REPLY_DELAY: Duration = Duration::from_millis(5);

#[test]
fn set_during_read_all_is_written_before_the_walk_ends() {
    let simulator = Simulator::start(SimModel::Kh750);
    let (handle, events) = start_worker(simulator.address(), None, quiet_timing());
    collect_until(&events, EVENT_DEADLINE, is_connected);
    simulator.set_reply_delay(WALK_REPLY_DELAY);

    handle.send(Command::ReadAll);
    wait_for_requests(&simulator, &path("audio/out/level"), 2, EVENT_DEADLINE);
    let level = path("audio/out/level");
    handle.send(Command::Set {
        path: level.clone(),
        value: json!(90.0),
    });
    let seen = collect_until(&events, RECOVERY_DEADLINE, |event| {
        matches!(event, Event::AllValues(_) | Event::AllValuesFailed(_))
    });
    let written = seen.iter().position(|event| {
        *event
            == Event::Value {
                path: level.clone(),
                value: json!(90.0),
            }
    });
    assert!(written.is_some(), "the set waited for the walk");
    assert!(matches!(seen.last(), Some(Event::AllValues(_))));
}

#[test]
fn meters_keep_running_during_read_all() {
    let simulator = Simulator::start(SimModel::Kh750);
    let timing = Timing {
        meter: Duration::from_millis(20),
        ..quiet_timing()
    };
    let (handle, events) = start_worker(simulator.address(), None, timing);
    collect_until(&events, EVENT_DEADLINE, is_connected);
    simulator.set_reply_delay(WALK_REPLY_DELAY);

    handle.send(Command::ReadAll);
    let meter = path("m/in/level");
    let seen = collect_until(&events, RECOVERY_DEADLINE, |event| {
        matches!(event, Event::AllValues(_) | Event::AllValuesFailed(_))
    });
    let meter_values = seen
        .iter()
        .filter(|event| is_value_of(event, &meter))
        .count();
    assert!(
        meter_values >= 5,
        "only {meter_values} meter values during the walk"
    );
}

#[test]
fn connection_loss_during_read_all_is_answered() {
    let simulator = Simulator::start(SimModel::Kh750);
    let timing = Timing {
        meter: Duration::from_millis(20),
        ..quiet_timing()
    };
    let (handle, events) = start_worker(simulator.address(), None, timing);
    collect_until(&events, EVENT_DEADLINE, is_connected);
    simulator.set_reply_delay(WALK_REPLY_DELAY);

    handle.send(Command::ReadAll);
    wait_for_requests(&simulator, &path("audio/out/level"), 2, EVENT_DEADLINE);
    simulator.inject(Fault::CloseConnection);
    let seen = collect_until(&events, EVENT_DEADLINE, |event| {
        matches!(event, Event::AllValues(_) | Event::AllValuesFailed(_))
    });
    assert!(matches!(seen.last(), Some(Event::AllValuesFailed(_))));
}

#[test]
fn polling_pauses_while_nobody_reads_the_events() {
    let simulator = Simulator::start(SimModel::Kh120Ii);
    let timing = Timing {
        unread_events_limit: 30,
        ..short_timing()
    };
    let (_handle, events) = start_worker(simulator.address(), None, timing);
    let meter = path("m/in/level");
    wait_for_requests(&simulator, &meter, 1, EVENT_DEADLINE);

    thread::sleep(QUIET_PERIOD);
    let paused_at = count_requests(&simulator, &meter);
    thread::sleep(QUIET_PERIOD);
    assert_eq!(
        count_requests(&simulator, &meter),
        paused_at,
        "polling went on"
    );

    while events.try_recv().is_ok() {}
    wait_for_requests(&simulator, &meter, paused_at + 3, EVENT_DEADLINE);
}

#[test]
fn response_time_is_reported_about_once_a_second() {
    let simulator = Simulator::start(SimModel::Kh120Ii);
    simulator.set_reply_delay(Duration::from_millis(30));
    let (_handle, events) = start_worker(simulator.address(), None, short_timing());
    collect_until(&events, EVENT_DEADLINE, is_connected);

    let end = Instant::now() + Duration::from_millis(2500);
    let mut times = Vec::new();
    while let Some(remaining) = end.checked_duration_since(Instant::now()) {
        if let Ok(Event::ResponseTime(time)) = events.recv_timeout(remaining) {
            times.push(time);
        }
    }
    assert!((2..=3).contains(&times.len()), "{times:?}");
    assert!(
        times.iter().all(|time| *time >= Duration::from_millis(25)),
        "{times:?}"
    );
}

#[test]
fn drop_stops_the_thread() {
    let simulator = Simulator::start(SimModel::Kh120Ii);
    let (handle, events) = start_worker(simulator.address(), None, short_timing());
    collect_until(&events, EVENT_DEADLINE, is_connected);

    let started = Instant::now();
    drop(handle);
    assert!(started.elapsed() < STOP_DEADLINE);
    // The worker owned the only sender; a finished thread leaves the channel closed.
    let _: Vec<Event> = events.try_iter().collect();
    assert_eq!(
        events.recv_timeout(Duration::ZERO),
        Err(RecvTimeoutError::Disconnected)
    );
}

#[test]
fn drop_stops_the_thread_while_waiting_to_reconnect() {
    let listener = ClosingListener::start();
    let timing = Timing {
        reconnect: LONG_INTERVAL,
        ..short_timing()
    };
    let (handle, events) = start_worker(listener.address, None, timing);
    collect_until(&events, EVENT_DEADLINE, is_unreachable);

    let started = Instant::now();
    drop(handle);
    assert!(started.elapsed() < STOP_DEADLINE);
}

#[test]
fn drop_during_read_all_returns_promptly() {
    let simulator = Simulator::start(SimModel::Kh120Ii);
    let (handle, events) = start_worker(simulator.address(), None, quiet_timing());
    let last = last_initial_path(KH_120_II_PRODUCT);
    collect_until(&events, EVENT_DEADLINE, |event| is_value_of(event, &last));

    simulator.set_reply_delay(SLOW_REPLY);
    handle.send(Command::ReadAll);
    wait_for_requests(&simulator, &path("osc/schema"), 1, EVENT_DEADLINE);

    let started = Instant::now();
    drop(handle);
    assert!(started.elapsed() < STOP_DEADLINE);
}

#[test]
fn drop_during_connect_read_returns_promptly() {
    let simulator = Simulator::start(SimModel::Kh120Ii);
    simulator.set_reply_delay(SLOW_REPLY);
    let (handle, events) = start_worker(simulator.address(), None, quiet_timing());
    collect_until(&events, EVENT_DEADLINE, is_connected);

    let started = Instant::now();
    drop(handle);
    assert!(started.elapsed() < STOP_DEADLINE);
}

#[test]
fn oversized_schema_fails_read_all_but_keeps_connection() {
    let simulator = Simulator::start(SimModel::Kh120Ii);
    let deep = (0..=MAX_SCHEMA_DEPTH)
        .map(|level| format!("level{level}"))
        .collect::<Vec<_>>()
        .join("/");
    simulator.set_value(&deep, json!(1));
    let (handle, events) = start_worker(simulator.address(), None, quiet_timing());
    collect_until(&events, EVENT_DEADLINE, is_connected);

    handle.send(Command::ReadAll);
    let seen = collect_until(&events, EVENT_DEADLINE, |event| {
        matches!(event, Event::AllValues(_) | Event::AllValuesFailed(_))
    });
    assert!(matches!(seen.last(), Some(Event::AllValuesFailed(_))));

    let mute = path("audio/out/mute");
    handle.send(Command::Set {
        path: mute.clone(),
        value: json!(true),
    });
    let seen = collect_until(&events, EVENT_DEADLINE, |event| {
        *event
            == Event::Value {
                path: mute.clone(),
                value: json!(true),
            }
    });
    assert!(!seen.iter().any(is_unreachable));
}

#[test]
fn too_many_schema_nodes_are_refused() {
    let simulator = Simulator::start(SimModel::Kh120Ii);
    for index in 0..=MAX_SCHEMA_NODES {
        simulator.set_value(&format!("wide/node{index}"), json!(index));
    }
    let mut client = Client::connect(simulator.address()).expect("simulator accepts");

    let outcome = read_all(&mut client, &|| false);

    assert!(matches!(outcome, Err(WalkError::Refused(_))));
    assert!(!client.is_closed());
}

#[test]
fn read_all_stops_when_asked() {
    let simulator = Simulator::start(SimModel::Kh120Ii);
    let mut client = Client::connect(simulator.address()).expect("simulator accepts");

    let outcome = read_all(&mut client, &|| true);

    assert_eq!(outcome, Err(WalkError::Stopped));
    assert!(simulator.requests().is_empty());
}

#[test]
fn commands_queued_during_long_work_keep_their_order() {
    let simulator = Simulator::start(SimModel::Kh120Ii);
    simulator.set_reply_delay(Duration::from_millis(20));
    let (handle, events) = start_worker(simulator.address(), None, quiet_timing());
    collect_until(&events, EVENT_DEADLINE, is_connected);

    // Sent while the connect-time read is still running, so the worker sets them aside.
    let level = path("audio/out/level");
    for value in [80.0, 90.0] {
        handle.send(Command::Set {
            path: level.clone(),
            value: json!(value),
        });
    }
    let last_set = Event::Value {
        path: level.clone(),
        value: json!(90.0),
    };
    let seen = collect_until(&events, RECOVERY_DEADLINE, |event| *event == last_set);
    let first_set = Event::Value {
        path: level,
        value: json!(80.0),
    };
    assert!(seen.contains(&first_set), "the first set did not run first");
    assert_eq!(simulator.value("audio/out/level"), Some(json!(90.0)));
}
