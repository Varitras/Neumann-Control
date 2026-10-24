use neumann_control_core::ssc::wire::{
    device_error, extract, number, parse_line, reply_xid, request_line, token_count, DeviceError,
    SscPath, WireError, MAX_REQUEST_TOKENS,
};
use serde_json::{json, Value};

const FIXTURES: &str = include_str!("fixtures/recorded_replies.json");

fn group(name: &str) -> Vec<(Value, String)> {
    let fixtures: Value = serde_json::from_str(FIXTURES).expect("fixture file is valid JSON");
    fixtures[name]
        .as_array()
        .unwrap_or_else(|| panic!("fixture group {name} is missing"))
        .iter()
        .map(|entry| {
            (
                entry["request"].clone(),
                entry["reply"]
                    .as_str()
                    .expect("reply is a string")
                    .to_owned(),
            )
        })
        .collect()
}

fn path(text: &str) -> SscPath {
    SscPath::parse(text).expect("valid path")
}

#[test]
fn nested_404_is_found_at_any_depth() {
    let replies: Vec<_> = group("not_found")
        .into_iter()
        .chain(group("kh750_not_found"))
        .chain(group("container"))
        .collect();
    assert!(replies.len() >= 5);
    for (_, reply) in replies {
        let tree = parse_line(reply.as_bytes()).unwrap();
        let error = device_error(&tree).unwrap_or_else(|| panic!("no error in {reply}"));
        assert_eq!(error.code, 404, "{reply}");
        assert_eq!(error.description, "address not found", "{reply}");
    }
}

#[test]
fn bundle_reply_keeps_value_and_error() {
    let (_, reply) = group("bundle").remove(0);
    let tree = parse_line(reply.as_bytes()).unwrap();
    assert_eq!(
        extract(&tree, &path("audio/out/level")),
        Some(&json!(100.0))
    );
    assert_eq!(device_error(&tree).unwrap().code, 404);
}

#[test]
fn duplicate_osc_members_are_merged() {
    let tree = parse_line(
        br#"{"osc":{"error":[{"a":[210,{"desc":"partial success"}]}]},"osc":{"xid":7}}"#,
    )
    .unwrap();
    assert_eq!(reply_xid(&tree), Some(7));
    assert_eq!(device_error(&tree).unwrap().code, 210);
}

#[test]
fn duplicate_non_object_member_takes_the_later_value() {
    let tree = parse_line(br#"{"a":1,"a":2,"b":{"c":1},"b":{"d":2}}"#).unwrap();
    assert_eq!(tree, json!({"a": 2, "b": {"c": 1, "d": 2}}));
}

#[test]
fn set_replies_report_their_errors() {
    let codes: Vec<Option<i64>> = group("set")
        .into_iter()
        .map(|(_, reply)| {
            let reply = parse_line(reply.as_bytes()).unwrap();
            device_error(&reply).map(|error| error.code)
        })
        .collect();
    assert!(codes.contains(&None));
    assert!(codes.contains(&Some(DeviceError::NOT_FOUND)));
    assert!(codes.contains(&Some(DeviceError::READ_ONLY)));
    assert!(codes.contains(&Some(DeviceError::NOT_ACCEPTABLE)));
}

#[test]
fn flat_error_pair_is_found() {
    let tree = parse_line(br#"{"osc":{"error":[404,{"desc":"address not found"}]}}"#).unwrap();
    assert_eq!(device_error(&tree).unwrap().code, 404);
}

#[test]
fn reply_without_error_has_none() {
    let tree = parse_line(br#"{"audio":{"out":{"mute":false}}}"#).unwrap();
    assert!(device_error(&tree).is_none());
}

#[test]
fn xid_is_read_from_recorded_echo() {
    let (_, reply) = group("xid").remove(0);
    let tree = parse_line(reply.as_bytes()).unwrap();
    assert_eq!(reply_xid(&tree), Some(4711));
}

#[test]
fn string_xid_is_not_a_number() {
    let (_, reply) = group("xid").remove(1);
    let tree = parse_line(reply.as_bytes()).unwrap();
    assert_eq!(reply_xid(&tree), None);
}

#[test]
fn extract_ignores_null_and_missing() {
    let tree = parse_line(br#"{"osc":{"ping":null}}"#).unwrap();
    assert_eq!(extract(&tree, &path("osc/ping")), None);
    assert_eq!(extract(&tree, &path("osc/version")), None);
}

#[test]
fn request_line_nests_path_and_xid() {
    let line = request_line(&path("audio/out/level"), Value::Null, 5);
    assert_eq!(
        parse_line(line.as_bytes()).unwrap(),
        json!({"osc": {"xid": 5}, "audio": {"out": {"level": null}}})
    );
    let line = request_line(&path("osc/schema"), Value::Null, 6);
    assert_eq!(
        parse_line(line.as_bytes()).unwrap(),
        json!({"osc": {"xid": 6, "schema": null}})
    );
    assert!(!line.contains('\n'));
}

#[test]
fn non_finite_number_is_refused() {
    assert_eq!(number(f64::NAN), Err(WireError::NonFinite));
    assert_eq!(number(f64::INFINITY), Err(WireError::NonFinite));
    assert_eq!(number(f64::NEG_INFINITY), Err(WireError::NonFinite));
    assert_eq!(number(1.5), Ok(json!(1.5)));
}

#[test]
fn non_object_and_garbage_lines_are_errors() {
    assert_eq!(parse_line(b"[1]"), Err(WireError::NotAnObject));
    assert_eq!(parse_line(b"\xff"), Err(WireError::Malformed));
    assert_eq!(parse_line(b"{"), Err(WireError::Malformed));
}

#[test]
fn path_parse_rejects_empty_segments() {
    assert_eq!(SscPath::parse("audio//level"), Err(WireError::InvalidPath));
    assert_eq!(SscPath::parse(""), Err(WireError::InvalidPath));
}

#[test]
fn path_displays_and_serializes_as_string() {
    let parsed = path("audio/out/level");
    assert_eq!(parsed.to_string(), "audio/out/level");
    assert_eq!(parsed.segments(), ["audio", "out", "level"]);
    assert_eq!(
        serde_json::to_string(&parsed).unwrap(),
        r#""audio/out/level""#
    );
    let back: SscPath = serde_json::from_str(r#""audio/out/level""#).unwrap();
    assert_eq!(back, parsed);
    assert!(serde_json::from_str::<SscPath>(r#""a//b""#).is_err());
}

/// Measured on a KH 120 II: the speaker refuses a request of more than 29 JSON tokens with 413,
/// and a write of a 20-band EQ list with the xid has 33. Without it the line fits; the reply is
/// then matched by its path.
#[test]
fn a_twenty_band_eq_write_leaves_out_the_xid() {
    let twenty = Value::Array(vec![Value::Null; 20]);
    let line = request_line(&path("audio/out/eq3/enabled"), twenty.clone(), 5);
    assert_eq!(
        parse_line(line.as_bytes()).unwrap(),
        json!({"audio": {"out": {"eq3": {"enabled": twenty}}}})
    );
    let ten = Value::Array(vec![Value::Null; 10]);
    let line = request_line(&path("audio/out/eq2/enabled"), ten.clone(), 6);
    assert_eq!(
        parse_line(line.as_bytes()).unwrap(),
        json!({"osc": {"xid": 6}, "audio": {"out": {"eq2": {"enabled": ten}}}})
    );
}

/// `audio/out/eq2/enabled` with n values and the xid is 13 + n tokens: 16 values are exactly
/// the 29 the speaker takes, 17 are one too many.
#[test]
fn the_xid_stays_up_to_exactly_the_token_limit() {
    let at_limit = Value::Array(vec![Value::Null; 16]);
    let line = request_line(&path("audio/out/eq2/enabled"), at_limit.clone(), 7);
    let request = parse_line(line.as_bytes()).unwrap();
    assert_eq!(token_count(&request), MAX_REQUEST_TOKENS);
    assert_eq!(reply_xid(&request), Some(7));
    let over = Value::Array(vec![Value::Null; 17]);
    let line = request_line(&path("audio/out/eq2/enabled"), over, 8);
    assert_eq!(reply_xid(&parse_line(line.as_bytes()).unwrap()), None);
}
