use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv6Addr, SocketAddrV6, TcpStream};
use std::time::Duration;

use neumann_control_core::simulator::{Fault, SimModel, Simulator};
use neumann_control_core::ssc::wire::{device_error, parse_line, reply_xid};
use serde_json::{json, Value};

const FIXTURES: &str = include_str!("fixtures/recorded_replies.json");
const READ_TIMEOUT: Duration = Duration::from_secs(3);
const SILENCE_TIMEOUT: Duration = Duration::from_millis(400);
// Longer than the simulator's read poll interval, so the first half is seen on its own.
const PARTIAL_LINE_PAUSE: Duration = Duration::from_millis(200);
const LINE_LIMIT_BYTES: usize = 1_048_576;
const FOREIGN_XID_OFFSET: u64 = 1000;
const KH_120_II: &str = "KH 120 II";
const KH_750: &str = "KH 750";
const KH_120_II_LEAVES: usize = 50;
const KH_750_LEAVES: usize = 129;

struct Client {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

impl Client {
    fn connect(simulator: &Simulator) -> Client {
        let stream = TcpStream::connect(simulator.address()).expect("simulator accepts");
        stream.set_read_timeout(Some(READ_TIMEOUT)).unwrap();
        Client {
            reader: BufReader::new(stream.try_clone().unwrap()),
            writer: stream,
        }
    }

    fn send(&mut self, request: &Value) {
        write!(self.writer, "{request}\r\n").expect("request is written");
    }

    fn read_raw_line(&mut self) -> Option<String> {
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(line.trim_end().to_owned()),
        }
    }

    fn read_tree(&mut self) -> Value {
        let line = self.read_raw_line().expect("a reply line");
        parse_line(line.as_bytes()).expect("reply is a JSON object")
    }

    fn ask(&mut self, request: &Value) -> Value {
        self.send(request);
        self.read_tree()
    }
}

fn model_of(name: &str) -> SimModel {
    match name {
        KH_120_II => SimModel::Kh120Ii,
        KH_750 => SimModel::Kh750,
        other => panic!("unknown model {other}"),
    }
}

fn start_kh120() -> Simulator {
    Simulator::start(SimModel::Kh120Ii)
}

/// The reply with its transaction id removed, and an `osc` object left empty by that dropped.
fn without_xid(mut tree: Value) -> Value {
    let Some(osc) = tree.get_mut("osc").and_then(Value::as_object_mut) else {
        return tree;
    };
    osc.remove("xid");
    if osc.is_empty() {
        tree.as_object_mut().unwrap().remove("osc");
    }
    tree
}

fn recorded_entries() -> Vec<(String, Value, String, &'static str)> {
    let fixtures: Value = serde_json::from_str(FIXTURES).expect("fixture file is valid JSON");
    let mut entries = Vec::new();
    for (group, members) in fixtures.as_object().unwrap() {
        for member in members.as_array().unwrap() {
            let model = match (group.as_str(), member["model"].as_str()) {
                (_, Some(KH_750)) | ("kh750_not_found", _) => KH_750,
                _ => KH_120_II,
            };
            entries.push((
                group.clone(),
                member["request"].clone(),
                member["reply"].as_str().unwrap().to_owned(),
                model,
            ));
        }
    }
    entries
}

#[test]
fn simulator_matches_recorded_replies() {
    let entries = recorded_entries();
    let groups: std::collections::BTreeSet<_> =
        entries.iter().map(|entry| entry.0.as_str()).collect();
    assert!(groups.contains("osc_methods") && groups.contains("schema"));
    for (group, request, recorded, model) in &entries {
        let simulator = Simulator::start(model_of(model));
        let reply = Client::connect(&simulator).ask(request);
        let expected = parse_line(recorded.as_bytes()).unwrap();
        assert_eq!(
            without_xid(reply),
            without_xid(expected),
            "{group} / {model}: {request}"
        );
    }
}

#[test]
fn simulator_echoes_xid() {
    let simulator = start_kh120();
    let mut client = Client::connect(&simulator);
    let numeric = client.ask(&json!({"osc":{"xid":4711},"audio":{"out":{"level":null}}}));
    assert_eq!(reply_xid(&numeric), Some(4711));
    let text = client.ask(&json!({"osc":{"xid":"abc"},"audio":{"out":{"mute":null}}}));
    assert_eq!(text["osc"]["xid"], json!("abc"));
    let without = client.ask(&json!({"audio":{"out":{"mute":null}}}));
    assert!(without.get("osc").is_none(), "no xid was asked for");
}

#[test]
fn error_reply_carries_xid_as_second_osc_member() {
    let simulator = start_kh120();
    let mut client = Client::connect(&simulator);
    client.send(&json!({"osc":{"xid":7},"audio":{"out":{"dimm":null}}}));
    let line = client.read_raw_line().unwrap();
    assert_eq!(line.matches("\"osc\"").count(), 2, "{line}");
    let tree = parse_line(line.as_bytes()).unwrap();
    assert_eq!(reply_xid(&tree), Some(7));
    assert_eq!(device_error(&tree).unwrap().code, 404);
}

#[test]
fn simulator_refuses_read_only_set_with_405() {
    let simulator = start_kh120();
    let mut client = Client::connect(&simulator);
    let reply = client.ask(&json!({"ui":{"input_gain":-3.0}}));
    let error = device_error(&reply).expect("an error reply");
    assert_eq!(
        (error.code, error.description.as_str()),
        (405, "method not allowed")
    );
    assert_eq!(simulator.value("ui/input_gain"), Some(json!(0.0)));
}

#[test]
fn simulator_refuses_standby_enabled_set_on_kh750_only() {
    let kh750 = Simulator::start(SimModel::Kh750);
    let refused = Client::connect(&kh750).ask(&json!({"device":{"standby":{"enabled":false}}}));
    assert_eq!(device_error(&refused).unwrap().code, 405);
    let kh120 = start_kh120();
    let accepted = Client::connect(&kh120).ask(&json!({"device":{"standby":{"enabled":false}}}));
    assert!(device_error(&accepted).is_none());
    assert_eq!(kh120.value("device/standby/enabled"), Some(json!(false)));
}

#[test]
fn valid_set_is_stored_and_echoed() {
    let simulator = start_kh120();
    let mut client = Client::connect(&simulator);
    let reply = client.ask(&json!({"audio":{"out":{"level":90}}}));
    assert_eq!(reply, json!({"audio":{"out":{"level":90.0}}}));
    assert_eq!(simulator.value("audio/out/level"), Some(json!(90.0)));
    let wrong_list_length = client.ask(&json!({"audio":{"out":{"eq2":{"enabled":[true]}}}}));
    assert_eq!(device_error(&wrong_list_length).unwrap().code, 406);
    let wrong_type = client.ask(&json!({"audio":{"out":{"mute":"x"}}}));
    assert_eq!(device_error(&wrong_type).unwrap().code, 406);
    assert_eq!(simulator.value("audio/out/mute"), Some(json!(false)));
}

#[test]
fn simulator_schema_root_lists_top_level() {
    let simulator = start_kh120();
    let reply = Client::connect(&simulator).ask(&json!({"osc":{"schema":null}}));
    let level = &reply["osc"]["schema"][0];
    for container in ["device", "osc", "audio", "m", "ui"] {
        assert_eq!(level[container], json!({}), "{container}");
    }
    assert_eq!(level["warnings"], Value::Null);
    assert_eq!(level.as_object().unwrap().len(), 6);
    let unknown = Client::connect(&simulator).ask(&json!({"osc":{"schema":[{"nowhere":null}]}}));
    assert_eq!(device_error(&unknown).unwrap().code, 454);
}

fn count_leaves(client: &mut Client, prefix: &[&str], level: &Value) -> usize {
    level
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, node)| {
            if !node.is_object() {
                return 1;
            }
            let mut path = prefix.to_vec();
            path.push(name);
            let request = path.iter().rev().fold(
                Value::Null,
                |inner, segment| json!({ (segment.to_string()): inner }),
            );
            let reply = client.ask(&json!({"osc":{"schema":[request]}}));
            let mut child = reply["osc"]["schema"][0].clone();
            for segment in &path {
                child = child[segment].clone();
            }
            count_leaves(client, &path, &child)
        })
        .sum()
}

#[test]
fn simulator_trees_have_the_measured_leaf_counts() {
    for (model, expected) in [
        (SimModel::Kh120Ii, KH_120_II_LEAVES),
        (SimModel::Kh750, KH_750_LEAVES),
    ] {
        let simulator = Simulator::start(model);
        let mut client = Client::connect(&simulator);
        let root = client.ask(&json!({"osc":{"schema":null}}))["osc"]["schema"][0].clone();
        let mut level = root.as_object().unwrap().clone();
        level.remove("osc");
        assert_eq!(
            count_leaves(&mut client, &[], &Value::Object(level)),
            expected
        );
    }
}

#[test]
fn simulator_reports_limits_type_and_identity() {
    let simulator = Simulator::start(SimModel::Kh750);
    let mut client = Client::connect(&simulator);
    let number = client.ask(&json!({"osc":{"limits":[{"audio":{"out":{"level":null}}}]}}));
    assert_eq!(
        number["osc"]["limits"][0]["audio"]["out"]["level"],
        json!([{"type":"Number"}])
    );
    let list = client.ask(&json!({"osc":{"limits":[{"m":{"in":{"clip":null}}}]}}));
    assert_eq!(
        list["osc"]["limits"][0]["m"]["in"]["clip"],
        json!([{"type":"Boolean"}])
    );
    let text = client.ask(&json!({"osc":{"limits":[{"device":{"name":null}}]}}));
    assert_eq!(
        text["osc"]["limits"][0]["device"]["name"],
        json!([{"type":"String"}])
    );
    assert_eq!(
        simulator.value("device/identity/serial"),
        Some(json!("SIM0007500"))
    );
    assert_eq!(
        simulator.value("device/identity/version"),
        Some(json!("2_1_2"))
    );
    assert_eq!(
        simulator.value("device/identity/hw_version"),
        Some(json!(1))
    );
}

#[test]
fn requests_are_recorded_parsed() {
    let simulator = start_kh120();
    let mut client = Client::connect(&simulator);
    let request = json!({"osc":{"xid":1},"audio":{"out":{"mute":null}}});
    client.ask(&request);
    assert_eq!(simulator.requests(), vec![request]);
}

#[test]
fn set_value_and_remove_value_change_the_tree() {
    let simulator = start_kh120();
    let mut client = Client::connect(&simulator);
    simulator.set_value("audio/out/level", json!(42.5));
    let changed = client.ask(&json!({"audio":{"out":{"level":null}}}));
    assert_eq!(changed["audio"]["out"]["level"], json!(42.5));
    simulator.remove_value("audio/out/level");
    let removed = client.ask(&json!({"audio":{"out":{"level":null}}}));
    assert_eq!(device_error(&removed).unwrap().code, 404);
    assert_eq!(simulator.value("audio/out/level"), None);
}

#[test]
fn fault_close_connection_ends_stream() {
    let simulator = start_kh120();
    let mut client = Client::connect(&simulator);
    simulator.inject(Fault::CloseConnection);
    client.send(&json!({"audio":{"out":{"mute":null}}}));
    assert_eq!(client.read_raw_line(), None);
    let mut second = Client::connect(&simulator);
    let healthy = second.ask(&json!({"audio":{"out":{"mute":null}}}));
    assert_eq!(
        healthy["audio"]["out"]["mute"],
        json!(false),
        "fault was spent"
    );
}

#[test]
fn fault_stale_line_comes_before_the_real_reply() {
    let simulator = start_kh120();
    let mut client = Client::connect(&simulator);
    simulator.inject(Fault::StaleLineFirst);
    client.send(&json!({"osc":{"xid":5},"audio":{"out":{"mute":null}}}));
    let stale = client.read_tree();
    assert_ne!(reply_xid(&stale), Some(5));
    let real = client.read_tree();
    assert_eq!(reply_xid(&real), Some(5));
    assert_eq!(real["audio"]["out"]["mute"], json!(false));
}

#[test]
fn fault_foreign_xid_comes_before_the_real_reply() {
    let simulator = start_kh120();
    let mut client = Client::connect(&simulator);
    simulator.inject(Fault::ForeignXidFirst);
    client.send(&json!({"osc":{"xid":5},"audio":{"out":{"mute":null}}}));
    assert_eq!(reply_xid(&client.read_tree()), Some(5 + FOREIGN_XID_OFFSET));
    assert_eq!(reply_xid(&client.read_tree()), Some(5));
}

#[test]
fn fault_no_xid_echo_drops_the_id_once() {
    let simulator = start_kh120();
    let mut client = Client::connect(&simulator);
    simulator.inject(Fault::NoXidEcho);
    let request = json!({"osc":{"xid":5},"audio":{"out":{"mute":null}}});
    assert_eq!(reply_xid(&client.ask(&request)), None);
    assert_eq!(reply_xid(&client.ask(&request)), Some(5));
}

#[test]
fn fault_overlong_line_exceeds_the_limit() {
    let simulator = start_kh120();
    let mut client = Client::connect(&simulator);
    simulator.inject(Fault::OverlongLine);
    client.send(&json!({"audio":{"out":{"mute":null}}}));
    let mut received = Vec::new();
    client
        .reader
        .by_ref()
        .take(LINE_LIMIT_BYTES as u64 + 1)
        .read_to_end(&mut received)
        .unwrap();
    assert_eq!(received.len(), LINE_LIMIT_BYTES + 1);
    assert!(!received.contains(&b'\n'));
}

#[test]
fn fault_silence_never_answers_that_request() {
    let simulator = start_kh120();
    let mut client = Client::connect(&simulator);
    simulator.inject(Fault::Silence);
    client.send(&json!({"audio":{"out":{"mute":null}}}));
    client
        .writer
        .set_read_timeout(Some(SILENCE_TIMEOUT))
        .unwrap();
    assert_eq!(client.read_raw_line(), None);
    assert_eq!(simulator.requests().len(), 1, "the request was read");
}

#[test]
fn stop_closes_connections_and_refuses_new_ones() {
    let simulator = start_kh120();
    let address = simulator.address();
    let mut client = Client::connect(&simulator);
    client.ask(&json!({"osc":{"ping":null}}));
    simulator.stop();
    assert_eq!(client.read_raw_line(), None, "open connection is closed");
    assert!(TcpStream::connect(address).is_err());
}

#[test]
fn start_on_rebinds_the_same_address() {
    let first = start_kh120();
    let address: SocketAddrV6 = first.address();
    assert_eq!(*address.ip(), Ipv6Addr::LOCALHOST);
    first.stop();
    let second = Simulator::start_on(SimModel::Kh120Ii, address).expect("port is free again");
    assert_eq!(second.address(), address);
    let reply = Client::connect(&second).ask(&json!({"osc":{"ping":null}}));
    assert_eq!(reply, json!({"osc":{"ping":null}}));
}

#[test]
fn two_connections_are_served_in_parallel() {
    let simulator = start_kh120();
    let mut first = Client::connect(&simulator);
    let mut second = Client::connect(&simulator);
    assert_eq!(
        first.ask(&json!({"osc":{"version":null}}))["osc"]["version"],
        json!("1.1")
    );
    assert_eq!(
        second.ask(&json!({"osc":{"version":null}}))["osc"]["version"],
        json!("1.1")
    );
}

#[test]
fn request_split_across_reads_is_reassembled() {
    let simulator = start_kh120();
    let mut client = Client::connect(&simulator);
    let request = json!({"audio":{"out":{"mute":null}}}).to_string();
    let (first, second) = request.split_at(request.len() / 2);
    client.writer.write_all(first.as_bytes()).unwrap();
    std::thread::sleep(PARTIAL_LINE_PAUSE);
    client
        .writer
        .write_all(format!("{second}\r\n").as_bytes())
        .unwrap();
    assert_eq!(client.read_tree()["audio"]["out"]["mute"], json!(false));
}
