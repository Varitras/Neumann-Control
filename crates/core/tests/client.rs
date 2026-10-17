use std::time::{Duration, Instant};

use neumann_control_core::simulator::{Fault, SimModel, Simulator};
use neumann_control_core::ssc::client::{Client, NodeKind, SscError};
use neumann_control_core::ssc::wire::SscPath;
use serde_json::{json, Value};

const SILENCE_DEADLINE: Duration = Duration::from_millis(3500);
const MISSING_PATH_CODE: i64 = 404;
const READ_ONLY_CODE: i64 = 405;
const WRONG_TYPE_CODE: i64 = 406;

fn path(text: &str) -> SscPath {
    SscPath::parse(text).expect("a valid path")
}

fn start(model: SimModel) -> (Simulator, Client) {
    let simulator = Simulator::start(model);
    let client = Client::connect(simulator.address()).expect("simulator accepts");
    (simulator, client)
}

fn device_code(error: SscError) -> i64 {
    match error {
        SscError::Device(device) => device.code,
        other => panic!("expected a device error, got {other:?}"),
    }
}

#[test]
fn get_returns_value() {
    let (_simulator, mut client) = start(SimModel::Kh750);
    assert_eq!(client.get(&path("audio/out/level")).unwrap(), json!(100.0));
}

#[test]
fn set_returns_confirmed_value() {
    let (simulator, mut client) = start(SimModel::Kh750);
    let confirmed = client.set(&path("audio/out/mute"), json!(true)).unwrap();
    assert_eq!(confirmed, json!(true));
    assert_eq!(simulator.value("audio/out/mute"), Some(json!(true)));
}

#[test]
fn missing_path_is_device_error_404() {
    let (_simulator, mut client) = start(SimModel::Kh750);
    let error = client.get(&path("audio/out5/level")).unwrap_err();
    assert_eq!(device_code(error), MISSING_PATH_CODE);
    assert!(!client.is_closed());
}

#[test]
fn read_only_set_is_device_error_405() {
    let (_simulator, mut client) = start(SimModel::Kh750);
    let error = client
        .set(&path("device/identity/serial"), json!("X"))
        .unwrap_err();
    assert_eq!(device_code(error), READ_ONLY_CODE);
}

#[test]
fn wrong_type_set_is_device_error_406() {
    let (_simulator, mut client) = start(SimModel::Kh750);
    let error = client
        .set(&path("audio/out/level"), json!("loud"))
        .unwrap_err();
    assert_eq!(device_code(error), WRONG_TYPE_CODE);
}

// A stale line without an id is accepted when it carries the requested path, so the stale
// line of the simulator (audio/out/level) must be skipped because it lacks a different path.
#[test]
fn stale_line_is_skipped_by_xid() {
    let (simulator, mut client) = start(SimModel::Kh750);
    simulator.inject(Fault::StaleLineFirst);
    let mute = client.get(&path("audio/out/mute")).unwrap();
    assert_eq!(mute, json!(false));
}

#[test]
fn foreign_xid_line_is_skipped() {
    let (simulator, mut client) = start(SimModel::Kh750);
    simulator.inject(Fault::ForeignXidFirst);
    assert_eq!(client.get(&path("audio/out/level")).unwrap(), json!(100.0));
    assert_eq!(client.get(&path("audio/out/mute")).unwrap(), json!(false));
}

#[test]
fn reply_without_xid_is_accepted_when_it_has_the_path() {
    let (simulator, mut client) = start(SimModel::Kh750);
    simulator.inject(Fault::NoXidEcho);
    assert_eq!(client.get(&path("audio/out/level")).unwrap(), json!(100.0));
}

#[test]
fn overlong_line_closes_connection() {
    let (simulator, mut client) = start(SimModel::Kh750);
    simulator.inject(Fault::OverlongLine);
    let error = client.get(&path("audio/out/level")).unwrap_err();
    assert!(matches!(error, SscError::Protocol(_)), "got {error:?}");
    assert!(client.is_closed());
}

#[test]
fn silent_device_times_out() {
    let (simulator, mut client) = start(SimModel::Kh750);
    simulator.inject(Fault::Silence);
    let started = Instant::now();
    let error = client.get(&path("audio/out/level")).unwrap_err();
    assert!(matches!(error, SscError::Timeout), "got {error:?}");
    assert!(started.elapsed() < SILENCE_DEADLINE);
    assert!(client.is_closed());
    assert!(error.is_connection_lost());
}

#[test]
fn closed_connection_is_connection_error() {
    let (simulator, mut client) = start(SimModel::Kh750);
    simulator.inject(Fault::CloseConnection);
    let error = client.get(&path("audio/out/level")).unwrap_err();
    assert!(matches!(error, SscError::Connection(_)), "got {error:?}");
    assert!(client.is_closed());
}

#[test]
fn closed_client_refuses_further_requests() {
    let (simulator, mut client) = start(SimModel::Kh750);
    simulator.inject(Fault::CloseConnection);
    let _ = client.get(&path("audio/out/level"));
    let error = client.get(&path("audio/out/level")).unwrap_err();
    assert!(matches!(error, SscError::Connection(_)), "got {error:?}");
}

#[test]
fn error_text_never_shows_the_interface_identifier() {
    let (simulator, mut client) = start(SimModel::Kh750);
    simulator.inject(Fault::CloseConnection);
    let error = client.get(&path("audio/out/level")).unwrap_err();
    let text = error.to_string();
    assert!(
        !text.contains(&simulator.address().ip().to_string()),
        "{text}"
    );
}

#[test]
fn schema_root_and_audio_level() {
    let (_simulator, mut client) = start(SimModel::Kh750);
    let root = client.schema(None).unwrap();
    assert!(root.contains(&("audio".to_owned(), NodeKind::Container)));
    let audio = client.schema(Some(&path("audio"))).unwrap();
    assert!(audio.contains(&("out1".to_owned(), NodeKind::Container)));
    assert!(audio.contains(&("out2".to_owned(), NodeKind::Container)));
    assert!(audio.contains(&("digital_bypass".to_owned(), NodeKind::Leaf)));
}

#[test]
fn limits_returns_the_dict() {
    let (_simulator, mut client) = start(SimModel::Kh750);
    let limits = client.limits(&path("audio/out/level")).unwrap();
    assert_eq!(limits["type"], json!("Number"));
}

#[test]
fn limits_of_unknown_path_is_device_error_454() {
    let (_simulator, mut client) = start(SimModel::Kh750);
    let error = client.limits(&path("audio/out5/level")).unwrap_err();
    assert_eq!(device_code(error), 454);
}

#[test]
fn non_finite_set_is_refused_before_sending() {
    let (simulator, mut client) = start(SimModel::Kh750);
    let before = simulator.requests().len();
    let error = client
        .set_number(&path("audio/out/level"), f64::NAN)
        .unwrap_err();
    assert!(matches!(error, SscError::Protocol(_)), "got {error:?}");
    let error = client
        .set_number(&path("audio/out/level"), f64::INFINITY)
        .unwrap_err();
    assert!(matches!(error, SscError::Protocol(_)), "got {error:?}");
    assert_eq!(simulator.requests().len(), before);
    assert!(!client.is_closed());
}

#[test]
fn set_number_sends_a_finite_number() {
    let (_simulator, mut client) = start(SimModel::Kh750);
    let confirmed: Value = client.set_number(&path("audio/out/level"), 90.0).unwrap();
    assert_eq!(confirmed, json!(90.0));
}
