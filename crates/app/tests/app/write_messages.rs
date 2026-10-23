//! A speaker's write messages across the seam worker -> events -> speaker state: a rejection
//! must still be shown after everything the worker sends in reply has been applied.

use std::sync::mpsc::RecvTimeoutError;
use std::sync::Arc;
use std::time::{Duration, Instant};

use neumann_control_app::state::{Message, SpeakerKey, SpeakerState, Status};
use neumann_control_app::writes::Intent;
use neumann_control_core::catalog::Catalog;
use neumann_control_core::simulator::{SimModel, Simulator};
use neumann_control_core::speaker::{self, Command, Event, Events, SpeakerHandle, Target, Timing};
use neumann_control_core::ssc::wire::SscPath;
use serde_json::json;

const EVENT_DEADLINE: Duration = Duration::from_secs(5);
/// No event for this long means the worker has answered everything it was asked.
const QUIET_PERIOD: Duration = Duration::from_millis(300);
/// Polls far apart, so only the reply to the write and its re-read follow it.
const LONG_INTERVAL: Duration = Duration::from_secs(60);
/// The error codes the speakers answer a refused write with.
const READ_ONLY_CODE: i64 = 405;
const LOCAL_MODE_CODE: i64 = 401;

fn quiet_timing() -> Timing {
    Timing {
        meter: LONG_INTERVAL,
        fast: LONG_INTERVAL,
        slow: LONG_INTERVAL,
        reconnect: LONG_INTERVAL,
        ..Timing::default()
    }
}

/// A worker for the simulator and the speaker state it feeds, once the worker is connected and
/// has read the values it reads on connecting.
fn connected(simulator: &Simulator) -> (SpeakerHandle, Events, SpeakerState) {
    let address = simulator.address();
    let target = Target {
        address,
        expected_serial: None,
    };
    let (handle, events) = speaker::spawn(target, quiet_timing(), Arc::new(|| {}));
    let mut state = SpeakerState::new(SpeakerKey::Address(address), address);
    apply_until(&events, &mut state, |event| {
        matches!(event, Event::Connected(_))
    });
    apply_until_quiet(&events, &mut state);
    assert_eq!(state.status, Status::Connected);
    (handle, events, state)
}

fn apply_until(events: &Events, state: &mut SpeakerState, done: impl Fn(&Event) -> bool) {
    let end = Instant::now() + EVENT_DEADLINE;
    loop {
        let remaining = end.saturating_duration_since(Instant::now());
        let event = events.recv_timeout(remaining).expect("the awaited event");
        let finished = done(&event);
        state.apply(event, 0.0);
        if finished {
            return;
        }
    }
}

fn apply_until_quiet(events: &Events, state: &mut SpeakerState) {
    loop {
        match events.recv_timeout(QUIET_PERIOD) {
            Ok(event) => state.apply(event, 0.0),
            Err(RecvTimeoutError::Timeout) => return,
            Err(RecvTimeoutError::Disconnected) => panic!("the worker ended"),
        }
    }
}

/// Applies the rejection of `path`, the worker's re-read of it and anything after that.
fn apply_reply_to_write(events: &Events, state: &mut SpeakerState, path: &SscPath) {
    apply_until(
        events,
        state,
        |event| matches!(event, Event::WriteRejected { path: rejected, .. } if rejected == path),
    );
    apply_until(
        events,
        state,
        |event| matches!(event, Event::Value { path: read, .. } if read == path),
    );
    apply_until_quiet(events, state);
}

#[test]
fn a_rejected_write_keeps_its_message_after_the_reread() {
    let simulator = Simulator::start(SimModel::Kh120Ii);
    let (handle, events, mut state) = connected(&simulator);
    let path = SscPath::parse("ui/bass_gain").unwrap();
    handle.send(Command::Set {
        path: path.clone(),
        value: json!(2.0),
    });
    apply_reply_to_write(&events, &mut state, &path);
    assert_eq!(state.message, Some(Message::Rejected(READ_ONLY_CODE)));
}

#[test]
fn a_write_in_local_mode_shows_the_local_mode_message() {
    let simulator = Simulator::start(SimModel::Kh120Ii);
    let (handle, events, mut state) = connected(&simulator);
    simulator.set_value("ui/control_mode", json!("LOCAL"));
    let command = state
        .request(Intent::Level(90.0), Catalog::embedded(), 0.0)
        .expect("the write policy allows the level");
    handle.send(command);
    let path = SscPath::parse("audio/out/level").unwrap();
    apply_reply_to_write(&events, &mut state, &path);
    assert_eq!(state.message, Some(Message::Rejected(LOCAL_MODE_CODE)));
    assert_eq!(simulator.value("audio/out/level"), Some(json!(100.0)));
}
