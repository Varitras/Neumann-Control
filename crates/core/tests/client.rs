use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv6Addr, SocketAddrV6, TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

use neumann_control_core::simulator::{Fault, SimModel, Simulator};
use neumann_control_core::ssc::client::{
    Client, NodeKind, SscError, MAX_LINE_BYTES, MAX_RESPONSE_BYTES, MAX_RESPONSE_LINES,
};
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

// Rule 3: a line without an id counts only if it has the requested path.
#[test]
fn xidless_line_without_the_path_is_skipped() {
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

const SHORT_LINE: &str = "{\"x\":1}
";
const BIG_LINE_BYTES: usize = 900 * 1024;
const BIG_LINE_COUNT: usize = 5;
const _: () = assert!(BIG_LINE_BYTES < MAX_LINE_BYTES);
const _: () = assert!(BIG_LINE_BYTES * BIG_LINE_COUNT > MAX_RESPONSE_BYTES);
const FOREIGN_XID: u64 = 999_999;
const TALKING_INTERVAL: Duration = Duration::from_secs(2);
const TALKING_LINES: usize = 8;
const TEN_SECOND_CUT_MIN: Duration = Duration::from_secs(9);
const TEN_SECOND_CUT_MAX: Duration = Duration::from_secs(13);

/// A one-connection fake speaker: reads the request line, then runs `script` with the
/// request's transaction id. Write errors in the script are ignored (the client hangs up).
fn scripted_speaker(script: impl FnOnce(&mut TcpStream, u64) + Send + 'static) -> Client {
    let listener = TcpListener::bind(SocketAddrV6::new(Ipv6Addr::LOCALHOST, 0, 0, 0)).unwrap();
    let address = match listener.local_addr().unwrap() {
        std::net::SocketAddr::V6(address) => address,
        other => panic!("not IPv6: {other}"),
    };
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = String::new();
        BufReader::new(stream.try_clone().unwrap())
            .read_line(&mut request)
            .unwrap();
        let tree: Value = serde_json::from_str(request.trim()).unwrap();
        let xid = tree["osc"]["xid"].as_u64().unwrap();
        script(&mut stream, xid);
    });
    Client::connect(address).expect("fake speaker accepts")
}

fn level_reply(xid: u64) -> String {
    format!(
        "{{\"audio\":{{\"out\":{{\"level\":1.0}}}},\"osc\":{{\"xid\":{xid}}}}}
"
    )
}

fn padded_object(total_content_bytes: usize) -> String {
    let overhead = "{\"x\":\"\"}".len();
    format!(
        "{{\"x\":\"{}\"}}",
        "a".repeat(total_content_bytes - overhead)
    )
}

fn assert_protocol_and_closed(mut client: Client) {
    let error = client.get(&path("audio/out/level")).unwrap_err();
    assert!(matches!(error, SscError::Protocol(_)), "got {error:?}");
    assert!(client.is_closed());
}

#[test]
fn too_many_lines_closes_connection() {
    let client = scripted_speaker(|stream, _| {
        for _ in 0..=MAX_RESPONSE_LINES {
            if stream.write_all(SHORT_LINE.as_bytes()).is_err() {
                break;
            }
        }
    });
    assert_protocol_and_closed(client);
}

#[test]
fn too_many_bytes_closes_connection() {
    let client = scripted_speaker(|stream, _| {
        let line = format!(
            "{}
",
            padded_object(BIG_LINE_BYTES)
        );
        for _ in 0..BIG_LINE_COUNT {
            if stream.write_all(line.as_bytes()).is_err() {
                break;
            }
        }
    });
    assert_protocol_and_closed(client);
}

#[test]
fn unterminated_stream_over_line_limit_closes_connection() {
    let client = scripted_speaker(|stream, _| {
        let chunk = vec![b'a'; MAX_LINE_BYTES + 1024];
        let _ = stream.write_all(&chunk);
    });
    assert_protocol_and_closed(client);
}

// The limit applies to the line without its CR LF: exactly MAX_LINE_BYTES is allowed.
#[test]
fn line_of_exactly_the_limit_is_accepted() {
    let mut client = scripted_speaker(|stream, xid| {
        let line = format!(
            "{}
",
            padded_object(MAX_LINE_BYTES)
        );
        let _ = stream.write_all(line.as_bytes());
        let _ = stream.write_all(level_reply(xid).as_bytes());
    });
    assert_eq!(client.get(&path("audio/out/level")).unwrap(), json!(1.0));
}

#[test]
fn line_one_byte_over_the_limit_closes_connection() {
    let client = scripted_speaker(|stream, _| {
        let line = format!(
            "{}
",
            padded_object(MAX_LINE_BYTES + 1)
        );
        let _ = stream.write_all(line.as_bytes());
    });
    assert_protocol_and_closed(client);
}

#[test]
fn device_that_keeps_talking_is_cut_after_ten_seconds() {
    let client = scripted_speaker(|stream, _| {
        let line = format!(
            "{{\"osc\":{{\"xid\":{FOREIGN_XID}}}}}
"
        );
        for _ in 0..TALKING_LINES {
            if stream.write_all(line.as_bytes()).is_err() {
                break;
            }
            thread::sleep(TALKING_INTERVAL);
        }
    });
    let started = Instant::now();
    assert_protocol_and_closed(client);
    let elapsed = started.elapsed();
    assert!(
        (TEN_SECOND_CUT_MIN..TEN_SECOND_CUT_MAX).contains(&elapsed),
        "cut after {elapsed:?}"
    );
}
